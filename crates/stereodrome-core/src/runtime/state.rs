use std::collections::BTreeMap;

use crate::protocol::{
    ConnectivityState, OperationFailure, OperationId, OperationSnapshot, PlatformLifecycle,
    RuntimeLifecycle, SavedPlaylistOfflineStatus,
};

pub(crate) struct CoreState {
    pub revision: u64,
    pub lifecycle: RuntimeLifecycle,
    pub connectivity: ConnectivityState,
    pub settings_revision: u64,
    pub library_revision: u64,
    pub last_failure: Option<OperationFailure>,
    pub operations: BTreeMap<OperationId, OperationSnapshot>,
    pub saved_playlist_offline: SavedPlaylistOfflineStatus,
    pub platform_lifecycle: PlatformLifecycle,
    pub network_available: bool,
}

impl CoreState {
    pub fn new(connectivity: ConnectivityState) -> Self {
        Self {
            revision: 0,
            lifecycle: RuntimeLifecycle::Starting,
            connectivity,
            settings_revision: 0,
            library_revision: 0,
            last_failure: None,
            operations: BTreeMap::new(),
            saved_playlist_offline: SavedPlaylistOfflineStatus::default(),
            platform_lifecycle: PlatformLifecycle::Foreground,
            network_available: true,
        }
    }
}
