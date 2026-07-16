use std::sync::Arc;

use log::{info, warn};

use crate::audio::loudness;
use crate::cache::AudioCache;
use crate::db;
use crate::error::{AppError, AppResult, MutexExt};
use crate::state::DesktopState;

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

pub fn get_analysis_progress(state: &DesktopState) -> Option<AnalysisProgress> {
    state.analysis_progress.lock_recover().clone()
}

pub fn get_normalization_stats(state: &DesktopState) -> AppResult<NormalizationStats> {
    let conn = state.db.lock_recover();
    let analyzed_count = conn
        .query_row("SELECT COUNT(*) FROM normalization_data", [], |row| {
            row.get(0)
        })
        .map_err(AppError::Database)?;
    let total_count = conn
        .query_row("SELECT COUNT(*) FROM songs", [], |row| row.get(0))
        .map_err(AppError::Database)?;
    Ok(NormalizationStats {
        analyzed_count,
        total_count,
    })
}

pub fn analyze_all_songs(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
) -> AppResult<()> {
    if state.analysis_progress.lock_recover().is_some() {
        return Err(AppError::Audio("Analysis already in progress".to_string()));
    }

    let (songs, total_count, already_analyzed) = {
        let conn = state.db.lock_recover();
        let total_count = conn
            .query_row("SELECT COUNT(*) FROM songs", [], |row| row.get(0))
            .map_err(AppError::Database)?;
        let already_analyzed = conn
            .query_row("SELECT COUNT(*) FROM normalization_data", [], |row| {
                row.get(0)
            })
            .map_err(AppError::Database)?;
        let mut statement = conn
            .prepare(
                "SELECT s.id, s.suffix, s.album_id FROM songs s
                 LEFT JOIN normalization_data n ON s.id = n.song_id
                 WHERE n.song_id IS NULL",
            )
            .map_err(AppError::Database)?;
        let songs = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(AppError::Database)?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        (songs, total_count, already_analyzed)
    };

    let total = songs.len() as i64;
    if total == 0 {
        return Ok(());
    }

    let progress_state = Arc::clone(&state.analysis_progress);
    *progress_state.lock_recover() = Some(AnalysisProgress {
        analyzed: 0,
        total,
        current_song: String::new(),
        analyzed_count: already_analyzed,
        total_count,
    });
    let task_runtime = runtime.clone();
    runtime.spawn(async move {
        let cache = match AudioCache::new(Arc::clone(&state)) {
            Ok(cache) => cache,
            Err(error) => {
                warn!("Failed to create audio cache for batch analysis: {error}");
                *progress_state.lock_recover() = None;
                return;
            }
        };
        let mut successful_writes = 0;

        for (index, (song_id, suffix, album_id)) in songs.into_iter().enumerate() {
            let audio_data = match cache.get_or_fetch(&state.client, &song_id, &suffix).await {
                Ok(data) => data,
                Err(error) => {
                    warn!("Failed to fetch audio for analysis of {song_id}: {error}");
                    continue;
                }
            };

            let analyzed_song_id = song_id.clone();
            let db_path = state.paths.database.clone();
            let analyzed = task_runtime
                .spawn_blocking(move || match loudness::analyze_loudness(audio_data) {
                    Ok(result) => {
                        let _ = db::save_normalization_result(
                            &db_path,
                            &analyzed_song_id,
                            &album_id,
                            result.integrated_lufs,
                            result.true_peak,
                        );
                        true
                    }
                    Err(error) => {
                        warn!("Failed to analyze loudness for {analyzed_song_id}: {error}");
                        false
                    }
                })
                .await;

            match analyzed {
                Ok(true) => successful_writes += 1,
                Ok(false) => {}
                Err(_) => warn!("Analysis task panicked for {song_id}"),
            }

            let progress = AnalysisProgress {
                analyzed: (index + 1) as i64,
                total,
                current_song: song_id,
                analyzed_count: already_analyzed + successful_writes,
                total_count,
            };
            *progress_state.lock_recover() = Some(progress.clone());
            state.events.normalization_progress(progress);
        }

        state.events.normalization_progress(AnalysisProgress {
            analyzed: total,
            total,
            current_song: String::new(),
            analyzed_count: already_analyzed + successful_writes,
            total_count,
        });
        *progress_state.lock_recover() = None;
        info!("Batch loudness analysis complete: {total} songs");
    });

    Ok(())
}

pub fn clear_normalization_data(state: &DesktopState) -> AppResult<()> {
    state
        .db
        .lock_recover()
        .execute("DELETE FROM normalization_data", [])
        .map_err(AppError::Database)?;
    Ok(())
}
