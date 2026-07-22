use log::{info, warn};
use tauri::{AppHandle, Emitter, State};

use crate::audio::loudness;
use crate::db;
use crate::error::{AppError, AppResult, MutexExt};
use crate::runtime::{deserialize_result, file_uri_path};
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

pub(crate) fn analyze_song_if_needed(
    runtime: stereodrome_core::StereodromeRuntimeHandle,
    db_path: std::path::PathBuf,
    song_id: String,
) {
    tauri::async_runtime::spawn_blocking(move || {
        match deserialize_result::<stereodrome_core::AudioProcessingSettings>(
            runtime.dispatch_command(stereodrome_core::CoreCommand::GetAudioProcessingSettings),
        ) {
            Ok(settings) if settings.normalization_enabled => {}
            Ok(_) => return,
            Err(error) => {
                warn!("Failed to read normalization settings for {song_id}: {error}");
                return;
            }
        }

        let album_id = match normalization_album_if_missing(&db_path, &song_id) {
            Ok(Some(album_id)) => album_id,
            Ok(None) => return,
            Err(error) => {
                warn!("Failed to inspect normalization state for {song_id}: {error}");
                return;
            }
        };
        let download: stereodrome_core::DownloadStatus = match deserialize_result(
            runtime.dispatch_command(stereodrome_core::CoreCommand::DownloadSong {
                song_id: song_id.clone(),
            }),
        ) {
            Ok(download) => download,
            Err(error) => {
                warn!("Failed to fetch audio for analysis of {song_id}: {error}");
                return;
            }
        };
        let Some(path) = download.path.as_deref().and_then(file_uri_path) else {
            warn!("Runtime download returned no local path for analysis of {song_id}");
            return;
        };
        let result = std::fs::read(path)
            .map_err(AppError::from)
            .and_then(|audio| {
                loudness::analyze_loudness(audio)
                    .map_err(|error| AppError::Audio(error.to_string()))
            });
        match result {
            Ok(result) => {
                if let Err(error) = db::save_normalization_result(
                    &db_path,
                    &song_id,
                    &album_id,
                    result.integrated_lufs,
                    result.true_peak,
                ) {
                    warn!("Failed to save loudness analysis for {song_id}: {error}");
                }
            }
            Err(error) => warn!("Failed to analyze loudness for {song_id}: {error}"),
        }
    });
}

fn normalization_album_if_missing(
    db_path: &std::path::Path,
    song_id: &str,
) -> AppResult<Option<String>> {
    use rusqlite::OptionalExtension;

    let conn = rusqlite::Connection::open(db_path)?;
    conn.query_row(
        "SELECT s.album_id FROM songs s
         WHERE s.id = ?1
           AND NOT EXISTS (SELECT 1 FROM normalization_data n WHERE n.song_id = s.id)",
        [song_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(AppError::Database)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_analysis_progress(state: State<'_, AppState>) -> Option<AnalysisProgress> {
    state.analysis_progress.lock_recover().clone()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_normalization_stats(state: State<'_, AppState>) -> AppResult<NormalizationStats> {
    let conn = rusqlite::Connection::open(&state.db_path)?;

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
#[allow(clippy::too_many_lines)]
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
        let conn = rusqlite::Connection::open(&state.db_path)?;

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
            .filter_map(std::result::Result::ok)
            .collect();

        (results, total_count, already_analyzed)
    };

    let total = i64::try_from(songs.len()).unwrap_or(i64::MAX);
    if total == 0 {
        return Ok(());
    }

    let runtime = state.runtime.clone();
    let progress_state = state.analysis_progress.clone();

    tauri::async_runtime::spawn(async move {
        let mut successful_writes: i64 = 0;

        for (i, (song_id, _suffix, album_id)) in songs.into_iter().enumerate() {
            let download: stereodrome_core::DownloadStatus = match deserialize_result(
                runtime.dispatch_command(stereodrome_core::CoreCommand::DownloadSong {
                    song_id: song_id.clone(),
                }),
            ) {
                Ok(download) => download,
                Err(e) => {
                    warn!("Failed to fetch audio for analysis of {song_id}: {e}");
                    continue;
                }
            };
            let Some(path) = download.path else {
                warn!("Runtime download returned no path for analysis of {song_id}");
                continue;
            };
            let Some(path) = file_uri_path(&path) else {
                warn!("Runtime download returned a non-file path for analysis of {song_id}");
                continue;
            };
            let audio_data = match std::fs::read(path) {
                Ok(data) => data,
                Err(e) => {
                    warn!("Failed to read audio for analysis of {song_id}: {e}");
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
                analyzed: i64::try_from(i.saturating_add(1)).unwrap_or(i64::MAX),
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
#[allow(clippy::needless_pass_by_value)]
pub fn clear_normalization_data(state: State<'_, AppState>) -> AppResult<()> {
    let conn = rusqlite::Connection::open(&state.db_path)?;
    conn.execute("DELETE FROM normalization_data", [])
        .map_err(AppError::Database)?;
    Ok(())
}
