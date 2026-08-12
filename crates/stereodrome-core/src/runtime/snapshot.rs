use crate::protocol::{CORE_PROTOCOL_VERSION, ConnectivityState, CoreSnapshot, DownloadSnapshot};
use crate::{ConnectionStatus, CoreResult, StereodromeCore};

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
