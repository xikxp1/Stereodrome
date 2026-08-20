use std::collections::HashSet;

use num_traits::ToPrimitive;

use crate::protocol::{
    CORE_PROTOCOL_VERSION, ConnectivityState, CoreSnapshot, DownloadSnapshot, JobKind,
};
use crate::{ConnectionStatus, CoreError, CoreResult, StereodromeCore};

use super::playback::{self, AudioPort};
use super::state::CoreState;

pub(crate) fn initial_connectivity(core: &StereodromeCore) -> CoreResult<ConnectivityState> {
    let status = core.get_connection_status()?;
    if core.manual_offline_enabled()? {
        return Ok(ConnectivityState::OfflineManual {
            server_url: status.server_url,
            username: status.username,
        });
    }
    Ok(disconnected_state(status))
}

pub(crate) fn connected_state(status: ConnectionStatus) -> ConnectivityState {
    match (status.server_url, status.username) {
        (Some(server_url), Some(username)) if status.connected => ConnectivityState::Online {
            server_url,
            username,
            server_version: status.server_version,
        },
        (Some(server_url), Some(username)) => ConnectivityState::Disconnected {
            server_url,
            username,
        },
        _ => ConnectivityState::Unconfigured,
    }
}

pub(crate) fn disconnected_state(status: ConnectionStatus) -> ConnectivityState {
    match (status.server_url, status.username) {
        (Some(server_url), Some(username)) => ConnectivityState::Disconnected {
            server_url,
            username,
        },
        _ => ConnectivityState::Unconfigured,
    }
}

pub(crate) fn build_snapshot(
    core: &StereodromeCore,
    audio: &dyn AudioPort,
    state: &CoreState,
) -> CoreResult<CoreSnapshot> {
    let mut downloading_song_ids = core.get_downloading_song_ids();
    downloading_song_ids.sort();
    let mut offline_song_ids = core.get_offline_song_ids()?;
    offline_song_ids.sort();
    let queue_length = download_queue_length(core, state, &offline_song_ids)?;

    Ok(CoreSnapshot {
        protocol_version: CORE_PROTOCOL_VERSION,
        revision: state.revision,
        lifecycle: state.lifecycle,
        connectivity: state.connectivity.clone(),
        playback: playback::projection(core, audio, state.playback_operation_id)?,
        queue: core.get_queue()?,
        sync: sync_snapshot(core, state)?,
        downloads: DownloadSnapshot {
            downloading_song_ids,
            offline_song_ids,
            queue_length,
        },
        operations: state.operations.values().cloned().collect(),
        saved_playlist_offline: state.saved_playlist_offline.clone(),
        platform_lifecycle: state.platform_lifecycle,
        network_available: state.network_available,
        settings_revision: state.settings_revision,
        library_revision: state.library_revision,
        last_failure: state.last_failure.clone(),
    })
}

fn download_queue_length(
    core: &StereodromeCore,
    state: &CoreState,
    offline_song_ids: &[String],
) -> CoreResult<usize> {
    let mut queued_song_ids = HashSet::new();

    for operation in state.operations.values() {
        match &operation.kind {
            JobKind::DownloadSong { song_id } => {
                queued_song_ids.insert(song_id.clone());
            }
            JobKind::DownloadAlbum { album_id } => {
                queued_song_ids.extend(
                    core.get_songs(Some(album_id.clone()), None)?
                        .into_iter()
                        .map(|song| song.id),
                );
            }
            JobKind::DownloadPlaylist { playlist_id } => {
                queued_song_ids.extend(core.playlist_song_ids(playlist_id)?);
            }
            JobKind::SavedPlaylistReconcile => {
                queued_song_ids.extend(core.saved_playlist_song_ids()?);
            }
            JobKind::QueuePrefetch => {
                let prefetch_count = core
                    .get_audio_processing_settings()?
                    .prefetch_count
                    .to_usize()
                    .ok_or_else(|| {
                        CoreError::InvalidInput("prefetch_count does not fit usize".to_string())
                    })?;
                queued_song_ids.extend(core.queue_prefetch_plan(prefetch_count)?.song_ids);
            }
            JobKind::Connect
            | JobKind::RestoreSession
            | JobKind::Sync { .. }
            | JobKind::BackgroundTick
            | JobKind::PlaybackPrepare { .. }
            | JobKind::ServerRequest => {}
        }
    }

    let offline_song_ids = offline_song_ids.iter().collect::<HashSet<_>>();
    Ok(queued_song_ids
        .iter()
        .filter(|song_id| !offline_song_ids.contains(song_id))
        .count())
}

fn sync_snapshot(
    core: &StereodromeCore,
    state: &CoreState,
) -> CoreResult<crate::LibrarySyncStatus> {
    let mut status = core.get_library_sync_status()?;
    for operation in state.operations.values() {
        let crate::JobKind::Sync { kind } = operation.kind else {
            continue;
        };
        match kind {
            crate::SyncKind::Full => {
                status.active_job = Some("full".to_string());
                status.full.running = true;
            }
            crate::SyncKind::Incremental => {
                status.active_job = Some("incremental".to_string());
                status.incremental.running = true;
            }
            crate::SyncKind::FullReconcile => {
                status.active_job = Some("full_reconcile".to_string());
                status.full_reconcile.running = true;
            }
        }
        break;
    }
    Ok(status)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "test setup and assertions intentionally fail fast"
)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::StereodromeCore;
    use crate::protocol::{
        CommandId, ConnectivityState, JobKind, OperationId, OperationPhase, OperationSnapshot,
    };

    use super::{CoreState, download_queue_length};

    #[test]
    fn download_queue_counts_unique_uncached_songs_in_active_jobs() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock follows Unix epoch")
            .as_nanos();
        let data_dir = std::env::temp_dir().join(format!(
            "stereodrome-download-queue-{}-{nonce}",
            std::process::id()
        ));
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let mut state = CoreState::new(ConnectivityState::Unconfigured);
        for operation_id in [1, 2] {
            state.operations.insert(
                OperationId(operation_id),
                OperationSnapshot {
                    operation_id: OperationId(operation_id),
                    cause_command_id: CommandId(operation_id),
                    kind: JobKind::DownloadSong {
                        song_id: "song-1".to_string(),
                    },
                    phase: OperationPhase::Running,
                },
            );
        }
        state.operations.insert(
            OperationId(3),
            OperationSnapshot {
                operation_id: OperationId(3),
                cause_command_id: CommandId(3),
                kind: JobKind::PlaybackPrepare {
                    song_id: "playback-song".to_string(),
                },
                phase: OperationPhase::Running,
            },
        );

        assert_eq!(
            download_queue_length(&core, &state, &[]).expect("queue length computes"),
            1
        );
        assert_eq!(
            download_queue_length(&core, &state, &["song-1".to_string()])
                .expect("cached songs are excluded"),
            0
        );

        drop(core);
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
