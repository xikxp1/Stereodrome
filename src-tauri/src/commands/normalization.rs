use crate::runtime::AppHandle;
use log::{info, warn};
use tauri::{Emitter, State};

use crate::audio::loudness;
use crate::cache::AudioCache;
use crate::db;
use crate::error::{AppError, AppResult, MutexExt};
use crate::state::AppState;

#[derive(Debug, Clone, serde::Serialize)]
pub struct NormalizationStats {
    pub analyzed_count: i64,
    pub total_count: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AnalysisProgress {
    pub analyzed: i64,
    pub total: i64,
    pub current_song: String,
    pub analyzed_count: i64,
    pub total_count: i64,
}

#[tauri::command]
pub fn get_analysis_progress(state: State<'_, AppState>) -> Option<AnalysisProgress> {
    state.analysis_progress.lock_recover().clone()
}

#[tauri::command]
pub fn get_normalization_stats(state: State<'_, AppState>) -> AppResult<NormalizationStats> {
    let conn = state.db.lock_recover();

    let analyzed_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM normalization_data", [], |row| {
            row.get(0)
        })
        .map_err(AppError::Database)?;

    let total_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM songs", [], |row| row.get(0))
        .map_err(AppError::Database)?;

    Ok(NormalizationStats {
        analyzed_count,
        total_count,
    })
}

#[tauri::command]
pub async fn analyze_all_songs(app_handle: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    // Check if analysis is already running
    {
        let progress = state.analysis_progress.lock_recover();
        if progress.is_some() {
            return Err(AppError::Audio("Analysis already in progress".to_string()));
        }
    }

    // Get all songs that haven't been analyzed yet, plus total counts
    let (songs, total_count, already_analyzed) = {
        let conn = state.db.lock_recover();

        let total_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM songs", [], |row| row.get(0))
            .map_err(AppError::Database)?;

        let already_analyzed: i64 = conn
            .query_row("SELECT COUNT(*) FROM normalization_data", [], |row| {
                row.get(0)
            })
            .map_err(AppError::Database)?;

        let mut stmt = conn
            .prepare(
                "SELECT s.id, s.suffix, s.album_id FROM songs s
                 LEFT JOIN normalization_data n ON s.id = n.song_id
                 WHERE n.song_id IS NULL",
            )
            .map_err(AppError::Database)?;

        let results: Vec<(String, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(AppError::Database)?
            .filter_map(|r| r.ok())
            .collect();

        (results, total_count, already_analyzed)
    };

    let total = songs.len() as i64;
    if total == 0 {
        return Ok(());
    }

    let client = state.client.clone();
    let progress_state = state.analysis_progress.clone();

    tauri::async_runtime::spawn(async move {
        let cache = match AudioCache::new(&app_handle) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to create audio cache for batch analysis: {e}");
                return;
            }
        };

        let mut successful_writes: i64 = 0;

        for (i, (song_id, suffix, album_id)) in songs.into_iter().enumerate() {
            // Fetch audio data
            let audio_data = match cache.get_or_fetch(&client, &song_id, &suffix).await {
                Ok(data) => data,
                Err(e) => {
                    warn!("Failed to fetch audio for analysis of {song_id}: {e}");
                    continue;
                }
            };

            // Analyze in a blocking thread
            let song_id_clone = song_id.clone();
            let album_id_clone = album_id.clone();
            let app_handle_clone = app_handle.clone();

            let result = tauri::async_runtime::spawn_blocking(move || {
                match loudness::analyze_loudness(audio_data) {
                    Ok(result) => {
                        if let Ok(db_path) = db::get_db_path(&app_handle_clone) {
                            let _ = db::save_normalization_result(
                                std::path::Path::new(&db_path),
                                &song_id_clone,
                                &album_id_clone,
                                result.integrated_lufs,
                                result.true_peak,
                            );
                        }
                        true
                    }
                    Err(e) => {
                        warn!("Failed to analyze loudness for {song_id_clone}: {e}");
                        false
                    }
                }
            })
            .await;

            match result {
                Ok(true) => successful_writes += 1,
                Ok(false) => {}
                Err(_) => warn!("Analysis task panicked for {song_id}"),
            }

            // Emit progress AFTER DB write completes
            let progress = AnalysisProgress {
                analyzed: (i + 1) as i64,
                total,
                current_song: song_id,
                analyzed_count: already_analyzed + successful_writes,
                total_count,
            };
            *progress_state.lock_recover() = Some(progress.clone());
            let _ = app_handle.emit("normalization-progress", progress);
        }

        // Emit completion and clear stored progress
        let completion = AnalysisProgress {
            analyzed: total,
            total,
            current_song: String::new(),
            analyzed_count: already_analyzed + successful_writes,
            total_count,
        };
        let _ = app_handle.emit("normalization-progress", completion);
        *progress_state.lock_recover() = None;

        info!("Batch loudness analysis complete: {total} songs");
    });

    Ok(())
}

#[tauri::command]
pub fn clear_normalization_data(state: State<'_, AppState>) -> AppResult<()> {
    let conn = state.db.lock_recover();
    conn.execute("DELETE FROM normalization_data", [])
        .map_err(AppError::Database)?;
    Ok(())
}
