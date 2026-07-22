//! Serialized runtime shell around the existing core services.

mod effect;
mod snapshot;
mod state;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;

use serde_json::Value;
use tokio::sync::broadcast;

use crate::protocol::{
    CORE_PROTOCOL_VERSION, CommandId, ConnectivityState, CoreCommand, CoreCommandRequest,
    CoreCommandResult, CoreEvent, CoreEventKind, OperationFailure, OperationId, ProtocolError,
    ProtocolErrorCode, RuntimeLifecycle,
};
use crate::{ConnectionStatus, CoreError, CoreResult, StereodromeCore};

use self::snapshot::{build_snapshot, connected_state, initial_connectivity};
use self::state::CoreState;

const MAILBOX_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 128;
const RESULT_CACHE_CAPACITY: usize = 256;
const GENERATED_COMMAND_ID_START: u64 = 1 << 63;

static ACTIVE_RUNTIME_PATHS: LazyLock<Mutex<HashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static NEXT_STREAM_ID: AtomicU64 = AtomicU64::new(1);

enum MailboxMessage {
    Dispatch {
        request: CoreCommandRequest,
        response: mpsc::Sender<CoreCommandResult>,
    },
    Stop,
}

struct RuntimeInner {
    mailbox: SyncSender<MailboxMessage>,
    events: broadcast::Sender<CoreEvent>,
    next_command_id: AtomicU64,
    stopped: AtomicBool,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Drop for RuntimeInner {
    fn drop(&mut self) {
        if !self.stopped.swap(true, Ordering::SeqCst) {
            let _ = self.mailbox.send(MailboxMessage::Stop);
        }
        if let Ok(thread) = self.thread.get_mut()
            && let Some(thread) = thread.take()
        {
            let _ = thread.join();
        }
    }
}

/// Cloneable entry point to the single serialized runtime mailbox.
#[derive(Clone)]
pub struct StereodromeRuntimeHandle {
    inner: Arc<RuntimeInner>,
}

impl StereodromeRuntimeHandle {
    /// Creates a core and starts the one runtime allowed to own `data_dir`.
    ///
    /// # Errors
    /// Returns an error if the core cannot initialize, a runtime already owns
    /// the directory, or the actor thread/runtime cannot start.
    pub fn start(data_dir: impl AsRef<Path>) -> CoreResult<Self> {
        let data_dir = data_dir.as_ref();
        let core = Arc::new(StereodromeCore::new(data_dir)?);
        Self::start_with_core(data_dir, core)
    }

    /// Starts a runtime around an existing core used by a platform adapter.
    ///
    /// # Errors
    /// Returns an error if a runtime already owns the directory or the actor
    /// thread/runtime cannot start.
    pub fn start_with_core(
        data_dir: impl AsRef<Path>,
        core: Arc<StereodromeCore>,
    ) -> CoreResult<Self> {
        let lease = RuntimeLease::acquire(data_dir.as_ref())?;
        let connectivity = initial_connectivity(&core)?;
        let tokio_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (mailbox, receiver) = mpsc::sync_channel(MAILBOX_CAPACITY);
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let actor_events = events.clone();
        let stream_id = NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed);
        let actor_thread = thread::Builder::new()
            .name("stereodrome-runtime".to_string())
            .spawn(move || {
                run_actor(
                    receiver,
                    actor_events,
                    stream_id,
                    core,
                    connectivity,
                    tokio_runtime,
                    lease,
                );
            })?;
        Ok(Self {
            inner: Arc::new(RuntimeInner {
                mailbox,
                events,
                next_command_id: AtomicU64::new(GENERATED_COMMAND_ID_START),
                stopped: AtomicBool::new(false),
                thread: Mutex::new(Some(actor_thread)),
            }),
        })
    }

    /// Sends an explicitly identified command through the bounded mailbox.
    #[must_use]
    pub fn dispatch(&self, request: CoreCommandRequest) -> CoreCommandResult {
        let command_id = request.command_id;
        if command_id.0 >= GENERATED_COMMAND_ID_START {
            return CoreCommandResult::failed(
                command_id,
                0,
                None,
                ProtocolError::new(
                    ProtocolErrorCode::InvalidCommandId,
                    "caller command_id must be less than 2^63",
                    false,
                ),
            );
        }
        self.send(request)
    }

    fn send(&self, request: CoreCommandRequest) -> CoreCommandResult {
        let command_id = request.command_id;
        if self.inner.stopped.load(Ordering::SeqCst) {
            return unavailable_result(command_id);
        }
        let (response_sender, response_receiver) = mpsc::channel();
        if self
            .inner
            .mailbox
            .send(MailboxMessage::Dispatch {
                request,
                response: response_sender,
            })
            .is_err()
        {
            return unavailable_result(command_id);
        }
        response_receiver
            .recv()
            .unwrap_or_else(|_| unavailable_result(command_id))
    }

    /// Assigns a command ID and sends a command through the bounded mailbox.
    #[must_use]
    pub fn dispatch_command(&self, command: CoreCommand) -> CoreCommandResult {
        let command_id = CommandId(self.inner.next_command_id.fetch_add(1, Ordering::Relaxed));
        self.send(CoreCommandRequest {
            protocol_version: CORE_PROTOCOL_VERSION,
            command_id,
            command,
        })
    }

    /// Returns a receiver for the runtime's single ordered event stream.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.inner.events.subscribe()
    }

    /// Requests an authoritative snapshot through the mailbox.
    #[must_use]
    pub fn snapshot(&self) -> CoreCommandResult {
        self.dispatch_command(CoreCommand::GetSnapshot)
    }

    /// Gracefully stops the actor and joins its thread.
    pub fn shutdown(&self) {
        if self.inner.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        let command_id = CommandId(self.inner.next_command_id.fetch_add(1, Ordering::Relaxed));
        let (response_sender, response_receiver) = mpsc::channel();
        let _ = self.inner.mailbox.send(MailboxMessage::Dispatch {
            request: CoreCommandRequest {
                protocol_version: CORE_PROTOCOL_VERSION,
                command_id,
                command: CoreCommand::Shutdown,
            },
            response: response_sender,
        });
        let _ = response_receiver.recv();
        if let Ok(mut thread) = self.inner.thread.lock()
            && let Some(thread) = thread.take()
        {
            let _ = thread.join();
        }
    }
}

struct RuntimeLease {
    path: PathBuf,
}

impl RuntimeLease {
    fn acquire(data_dir: &Path) -> CoreResult<Self> {
        std::fs::create_dir_all(data_dir)?;
        let path = std::fs::canonicalize(data_dir)?;
        let mut active = ACTIVE_RUNTIME_PATHS
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?;
        if !active.insert(path.clone()) {
            return Err(CoreError::InvalidInput(format!(
                "a Stereodrome runtime already owns {}",
                path.display()
            )));
        }
        Ok(Self { path })
    }
}

impl Drop for RuntimeLease {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE_RUNTIME_PATHS.lock() {
            active.remove(&self.path);
        }
    }
}

struct ResultCache {
    order: VecDeque<CommandId>,
    values: HashMap<CommandId, CachedResult>,
}

struct CachedResult {
    command_fingerprint: String,
    result: CoreCommandResult,
}

enum CacheLookup {
    Missing,
    Match(CoreCommandResult),
    Conflict,
}

impl ResultCache {
    fn new() -> Self {
        Self {
            order: VecDeque::with_capacity(RESULT_CACHE_CAPACITY),
            values: HashMap::with_capacity(RESULT_CACHE_CAPACITY),
        }
    }

    fn get(&self, command_id: CommandId, command: &CoreCommand) -> CacheLookup {
        let Some(cached) = self.values.get(&command_id) else {
            return CacheLookup::Missing;
        };
        let fingerprint = command_fingerprint(command);
        if cached.command_fingerprint == fingerprint {
            CacheLookup::Match(cached.result.clone())
        } else {
            CacheLookup::Conflict
        }
    }

    fn insert(&mut self, command: &CoreCommand, result: CoreCommandResult) {
        if self.values.contains_key(&result.command_id) {
            return;
        }
        if self.order.len() == RESULT_CACHE_CAPACITY
            && let Some(oldest) = self.order.pop_front()
        {
            self.values.remove(&oldest);
        }
        self.order.push_back(result.command_id);
        self.values.insert(
            result.command_id,
            CachedResult {
                command_fingerprint: command_fingerprint(command),
                result,
            },
        );
    }
}

fn command_fingerprint(command: &CoreCommand) -> String {
    serde_json::to_string(command).unwrap_or_else(|_| format!("{command:?}"))
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn run_actor(
    receiver: mpsc::Receiver<MailboxMessage>,
    events: broadcast::Sender<CoreEvent>,
    stream_id: u64,
    core: Arc<StereodromeCore>,
    connectivity: ConnectivityState,
    tokio_runtime: tokio::runtime::Runtime,
    _lease: RuntimeLease,
) {
    let mut state = CoreState::new(connectivity);
    state.lifecycle = RuntimeLifecycle::Ready;
    let mut next_operation_id = 1_u64;
    let mut next_event_id = 1_u64;
    let mut result_cache = ResultCache::new();

    while let Ok(message) = receiver.recv() {
        let MailboxMessage::Dispatch { request, response } = message else {
            break;
        };
        match result_cache.get(request.command_id, &request.command) {
            CacheLookup::Match(cached) => {
                let _ = response.send(cached);
                continue;
            }
            CacheLookup::Conflict => {
                let _ = response.send(CoreCommandResult::failed(
                    request.command_id,
                    state.revision,
                    None,
                    ProtocolError::new(
                        ProtocolErrorCode::Conflict,
                        "command_id was already used for a different command",
                        false,
                    ),
                ));
                continue;
            }
            CacheLookup::Missing => {}
        }

        let should_stop = matches!(request.command, CoreCommand::Shutdown);
        let command_for_cache = request.command.clone();
        let result = process_request(
            request,
            &core,
            &tokio_runtime,
            &events,
            stream_id,
            &mut state,
            &mut next_operation_id,
            &mut next_event_id,
        );
        result_cache.insert(&command_for_cache, result.clone());
        let _ = response.send(result);
        if should_stop {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn process_request(
    request: CoreCommandRequest,
    core: &StereodromeCore,
    tokio_runtime: &tokio::runtime::Runtime,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_operation_id: &mut u64,
    next_event_id: &mut u64,
) -> CoreCommandResult {
    if request.protocol_version != CORE_PROTOCOL_VERSION {
        return CoreCommandResult::failed(
            request.command_id,
            state.revision,
            None,
            ProtocolError::new(
                ProtocolErrorCode::UnsupportedProtocolVersion,
                format!(
                    "unsupported protocol version {}; expected {CORE_PROTOCOL_VERSION}",
                    request.protocol_version
                ),
                false,
            ),
        );
    }
    if request.command_id.0 == 0 {
        return CoreCommandResult::failed(
            request.command_id,
            state.revision,
            None,
            ProtocolError::new(
                ProtocolErrorCode::InvalidCommandId,
                "command_id must be greater than zero",
                false,
            ),
        );
    }

    let is_mutation = request.command.is_mutation();
    let changes_settings = request.command.changes_settings();
    let changes_library = request.command.changes_library();
    let operation_id = is_mutation.then(|| {
        let id = OperationId(*next_operation_id);
        *next_operation_id = next_operation_id.wrapping_add(1);
        id
    });

    if matches!(request.command, CoreCommand::Shutdown) {
        state.lifecycle = RuntimeLifecycle::ShuttingDown;
        state.revision = state.revision.wrapping_add(1);
        emit_event(
            events,
            stream_id,
            next_event_id,
            state.revision,
            request.command_id,
            operation_id,
            CoreEventKind::RuntimeShuttingDown,
        );
        return CoreCommandResult::succeeded(
            request.command_id,
            state.revision,
            operation_id,
            Value::Null,
        );
    }

    if matches!(
        request.command,
        CoreCommand::Initialize | CoreCommand::GetSnapshot
    ) {
        return match build_snapshot(core, state).and_then(to_value) {
            Ok(value) => CoreCommandResult::succeeded(
                request.command_id,
                state.revision,
                operation_id,
                value,
            ),
            Err(error) => CoreCommandResult::failed(
                request.command_id,
                state.revision,
                operation_id,
                ProtocolError::from(&error),
            ),
        };
    }

    let command = request.command;
    let command_for_state = command.clone();
    match tokio_runtime.block_on(effect::execute(core, command)) {
        Ok(value) => {
            if is_mutation {
                state.revision = state.revision.wrapping_add(1);
                state.last_failure = None;
                if changes_settings {
                    state.settings_revision = state.settings_revision.wrapping_add(1);
                }
                let due_sync_changed_library =
                    !matches!(command_for_state, CoreCommand::RunDueLibrarySync)
                        || !value.is_null();
                if changes_library && due_sync_changed_library {
                    state.library_revision = state.library_revision.wrapping_add(1);
                }
                update_connectivity(state, core, &command_for_state, &value);

                if let Ok(snapshot) = build_snapshot(core, state) {
                    emit_event(
                        events,
                        stream_id,
                        next_event_id,
                        state.revision,
                        request.command_id,
                        operation_id,
                        CoreEventKind::SnapshotChanged {
                            snapshot: Box::new(snapshot),
                        },
                    );
                }
            }
            CoreCommandResult::succeeded(request.command_id, state.revision, operation_id, value)
        }
        Err(error) => {
            let protocol_error = ProtocolError::from(&error);
            if is_mutation {
                state.revision = state.revision.wrapping_add(1);
                let failure = OperationFailure {
                    command_id: request.command_id,
                    operation_id,
                    error: protocol_error.clone(),
                };
                state.last_failure = Some(failure.clone());
                emit_event(
                    events,
                    stream_id,
                    next_event_id,
                    state.revision,
                    request.command_id,
                    operation_id,
                    CoreEventKind::OperationFailed { failure },
                );
            }
            CoreCommandResult::failed(
                request.command_id,
                state.revision,
                operation_id,
                protocol_error,
            )
        }
    }
}

fn update_connectivity(
    state: &mut CoreState,
    core: &StereodromeCore,
    command: &CoreCommand,
    value: &Value,
) {
    match command {
        CoreCommand::Connect { .. }
        | CoreCommand::UpdateServerSettings { .. }
        | CoreCommand::RestoreSession => {
            if let Ok(status) = serde_json::from_value::<ConnectionStatus>(value.clone()) {
                state.connectivity = connected_state(status);
            }
        }
        CoreCommand::Disconnect => state.connectivity = ConnectivityState::Unconfigured,
        CoreCommand::SetConnectivity { settings } if settings.manual_offline_enabled => {
            let status = core
                .get_connection_status()
                .unwrap_or_else(|_| ConnectionStatus::disconnected());
            state.connectivity = ConnectivityState::OfflineManual {
                server_url: status.server_url,
                username: status.username,
            };
        }
        CoreCommand::SetConnectivity { .. } | CoreCommand::ImportPortableBackup { .. } => {
            if let Ok(connectivity) = initial_connectivity(core) {
                state.connectivity = connectivity;
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_event(
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    next_event_id: &mut u64,
    revision: u64,
    command_id: CommandId,
    operation_id: Option<OperationId>,
    kind: CoreEventKind,
) {
    let event = CoreEvent {
        protocol_version: CORE_PROTOCOL_VERSION,
        stream_id,
        event_id: *next_event_id,
        revision,
        cause_command_id: command_id,
        operation_id,
        kind,
    };
    *next_event_id = next_event_id.wrapping_add(1);
    let _ = events.send(event);
}

fn to_value(value: impl serde::Serialize) -> CoreResult<Value> {
    serde_json::to_value(value).map_err(CoreError::from)
}

fn unavailable_result(command_id: CommandId) -> CoreCommandResult {
    CoreCommandResult::failed(
        command_id,
        0,
        None,
        ProtocolError::new(
            ProtocolErrorCode::RuntimeUnavailable,
            "runtime mailbox is unavailable",
            true,
        ),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::protocol::{CommandStatus, CoreCommand, CoreCommandRequest};
    use crate::queue::QueueItem;

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "stereodrome-runtime-{name}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn queue_item(id: u64) -> QueueItem {
        QueueItem {
            song_id: format!("song-{id}"),
            title: format!("Song {id}"),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration: 180,
        }
    }

    #[test]
    fn protocol_request_round_trips() {
        let request = CoreCommandRequest {
            protocol_version: CORE_PROTOCOL_VERSION,
            command_id: CommandId(7),
            command: CoreCommand::SetConnectivity {
                settings: crate::ConnectivitySettings {
                    manual_offline_enabled: true,
                },
            },
        };
        let json = serde_json::to_value(&request).expect("request serializes");
        assert_eq!(json["command"]["type"], "set-connectivity");
        let decoded: CoreCommandRequest =
            serde_json::from_value(json).expect("request deserializes");
        assert_eq!(decoded.protocol_version, CORE_PROTOCOL_VERSION);
        assert_eq!(decoded.command_id, CommandId(7));
    }

    #[test]
    fn mailbox_serializes_mutations_and_events() {
        let data_dir = test_dir("serialized");
        let handle = StereodromeRuntimeHandle::start(&data_dir).expect("runtime starts");
        let mut events = handle.subscribe();
        let handles: Vec<_> = (0..12)
            .map(|id| {
                let handle = handle.clone();
                thread::spawn(move || {
                    handle.dispatch_command(CoreCommand::AddToQueue {
                        item: queue_item(id),
                    })
                })
            })
            .collect();
        let mut revisions = Vec::new();
        for task in handles {
            let result = task.join().expect("sender does not panic");
            assert_eq!(result.status, CommandStatus::Succeeded);
            revisions.push(result.accepted_revision);
        }
        revisions.sort_unstable();
        assert_eq!(revisions, (1..=12).collect::<Vec<_>>());

        let mut event_revisions = Vec::new();
        while event_revisions.len() < 12 {
            event_revisions.push(events.try_recv().expect("one event per mutation").revision);
        }
        assert_eq!(event_revisions, (1..=12).collect::<Vec<_>>());

        let snapshot = handle.snapshot();
        assert_eq!(snapshot.status, CommandStatus::Succeeded);
        assert_eq!(
            snapshot.value.as_ref().unwrap()["queue"]["items"]
                .as_array()
                .unwrap()
                .len(),
            12
        );
        handle.shutdown();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn repeated_command_id_returns_cached_result() {
        let data_dir = test_dir("deduplicate");
        let handle = StereodromeRuntimeHandle::start(&data_dir).expect("runtime starts");
        let request = CoreCommandRequest {
            protocol_version: CORE_PROTOCOL_VERSION,
            command_id: CommandId(44),
            command: CoreCommand::AddToQueue {
                item: queue_item(1),
            },
        };
        let first = handle.dispatch(request.clone());
        let second = handle.dispatch(request);
        assert_eq!(first.accepted_revision, second.accepted_revision);
        assert_eq!(
            handle.snapshot().value.unwrap()["queue"]["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        handle.shutdown();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn reused_command_id_with_different_payload_is_rejected() {
        let data_dir = test_dir("command-id-conflict");
        let handle = StereodromeRuntimeHandle::start(&data_dir).expect("runtime starts");
        let first = handle.dispatch(CoreCommandRequest {
            protocol_version: CORE_PROTOCOL_VERSION,
            command_id: CommandId(12),
            command: CoreCommand::AddToQueue {
                item: queue_item(1),
            },
        });
        let conflict = handle.dispatch(CoreCommandRequest {
            protocol_version: CORE_PROTOCOL_VERSION,
            command_id: CommandId(12),
            command: CoreCommand::AddToQueue {
                item: queue_item(2),
            },
        });

        assert_eq!(first.status, CommandStatus::Succeeded);
        assert_eq!(conflict.status, CommandStatus::Failed);
        assert!(matches!(
            conflict.error,
            Some(ProtocolError {
                code: ProtocolErrorCode::Conflict,
                ..
            })
        ));
        assert_eq!(
            handle.snapshot().value.unwrap()["queue"]["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        handle.shutdown();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn compatibility_and_caller_command_ids_use_separate_ranges() {
        let data_dir = test_dir("command-id-ranges");
        let handle = StereodromeRuntimeHandle::start(&data_dir).expect("runtime starts");
        let generated = handle.dispatch_command(CoreCommand::AddToQueue {
            item: queue_item(1),
        });
        let caller = handle.dispatch(CoreCommandRequest {
            protocol_version: CORE_PROTOCOL_VERSION,
            command_id: CommandId(1),
            command: CoreCommand::AddToQueue {
                item: queue_item(2),
            },
        });

        assert!(generated.command_id.0 >= GENERATED_COMMAND_ID_START);
        assert_eq!(caller.command_id, CommandId(1));
        assert_eq!(caller.status, CommandStatus::Succeeded);
        assert_eq!(caller.accepted_revision, 2);
        handle.shutdown();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn only_one_runtime_can_own_a_data_directory() {
        let data_dir = test_dir("single-owner");
        let core = Arc::new(StereodromeCore::new(&data_dir).expect("core initializes"));
        let first = StereodromeRuntimeHandle::start_with_core(&data_dir, Arc::clone(&core))
            .expect("first runtime starts");
        let second = StereodromeRuntimeHandle::start_with_core(&data_dir, core);
        assert!(matches!(second, Err(CoreError::InvalidInput(_))));
        first.shutdown();
        let replacement = StereodromeRuntimeHandle::start(&data_dir).expect("lease is released");
        replacement.shutdown();
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
