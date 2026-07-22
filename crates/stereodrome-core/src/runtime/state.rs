use crate::protocol::{ConnectivityState, OperationFailure, RuntimeLifecycle};

pub(crate) struct CoreState {
    pub revision: u64,
    pub lifecycle: RuntimeLifecycle,
    pub connectivity: ConnectivityState,
    pub settings_revision: u64,
    pub library_revision: u64,
    pub last_failure: Option<OperationFailure>,
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
        }
    }
}
