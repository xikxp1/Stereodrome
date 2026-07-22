use crate::protocol::{CORE_PROTOCOL_VERSION, ConnectivityState, CoreSnapshot, DownloadSnapshot};
use crate::{ConnectionStatus, CoreResult, StereodromeCore};

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
        playback: core.get_playback_state()?,
        queue: core.get_queue()?,
        sync: core.get_library_sync_status()?,
        downloads: DownloadSnapshot {
            downloading_song_ids,
            offline_song_ids,
        },
        settings_revision: state.settings_revision,
        library_revision: state.library_revision,
        last_failure: state.last_failure.clone(),
    })
}
