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
use std::time::Duration;

use serde_json::Value;
use tokio::sync::broadcast;
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    CORE_PROTOCOL_VERSION, CommandId, ConnectivityState, CoreCommand, CoreCommandRequest,
    CoreCommandResult, CoreEvent, CoreEventKind, JobKind, OperationFailure, OperationId,
    OperationPhase, OperationSnapshot, ProtocolError, ProtocolErrorCode, RuntimeLifecycle,
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
    EffectCompleted {
        operation_id: OperationId,
        result: CoreResult<Value>,
    },
    Stop,
}

struct PendingEffect {
    command_id: CommandId,
    command: CoreCommand,
    response: Option<mpsc::Sender<CoreCommandResult>>,
    cancellation: CancellationToken,
    abort_handle: AbortHandle,
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
        let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("stereodrome-effect")
            .enable_all()
            .build()?;
        let (mailbox, receiver) = mpsc::sync_channel(MAILBOX_CAPACITY);
        let completion_mailbox = mailbox.clone();
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let actor_events = events.clone();
        let stream_id = NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed);
        let actor_thread = thread::Builder::new()
            .name("stereodrome-runtime".to_string())
            .spawn(move || {
                run_actor(
                    receiver,
                    completion_mailbox,
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

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
fn run_actor(
    receiver: mpsc::Receiver<MailboxMessage>,
    completion_mailbox: SyncSender<MailboxMessage>,
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
    let mut pending_effects = HashMap::<OperationId, PendingEffect>::new();

    while let Ok(message) = receiver.recv() {
        let MailboxMessage::Dispatch { request, response } = message else {
            match message {
                MailboxMessage::EffectCompleted {
                    operation_id,
                    result,
                } => complete_effect(
                    operation_id,
                    result,
                    &core,
                    &events,
                    stream_id,
                    &mut state,
                    &mut next_event_id,
                    &mut pending_effects,
                    &mut result_cache,
                ),
                MailboxMessage::Stop => break,
                MailboxMessage::Dispatch { .. } => unreachable!(),
            }
            continue;
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
        if let CoreCommand::CancelOperation { operation_id } = &request.command {
            let result = cancel_operation(
                request.command_id,
                *operation_id,
                &core,
                &events,
                stream_id,
                &mut state,
                &mut next_event_id,
                &mut pending_effects,
                &mut result_cache,
            );
            result_cache.insert(&request.command, result.clone());
            let _ = response.send(result);
            continue;
        }
        if let CoreCommand::CancelQueuePrefetch {
            invalidate_completed: _,
        } = &request.command
        {
            let prefetch = pending_effects.iter().find_map(|(id, pending)| {
                matches!(pending.command, CoreCommand::StartQueuePrefetch { .. }).then_some(*id)
            });
            if let Some(operation_id) = prefetch {
                let _ = cancel_operation(
                    request.command_id,
                    operation_id,
                    &core,
                    &events,
                    stream_id,
                    &mut state,
                    &mut next_event_id,
                    &mut pending_effects,
                    &mut result_cache,
                );
            }
            state.revision = state.revision.wrapping_add(1);
            let result =
                CoreCommandResult::succeeded(request.command_id, state.revision, None, Value::Null);
            emit_snapshot_event(
                &core,
                &events,
                stream_id,
                &mut next_event_id,
                &state,
                request.command_id,
                None,
            );
            result_cache.insert(&request.command, result.clone());
            let _ = response.send(result);
            continue;
        }
        if request.command.runs_as_effect() {
            start_effect(
                request,
                response,
                &core,
                &tokio_runtime,
                &completion_mailbox,
                &events,
                stream_id,
                &mut state,
                &mut next_operation_id,
                &mut next_event_id,
                &mut pending_effects,
                &mut result_cache,
            );
            continue;
        }
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

    cancel_all_effects(&mut pending_effects, state.revision);
    tokio_runtime.shutdown_timeout(Duration::from_secs(2));
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
fn start_effect(
    request: CoreCommandRequest,
    response: mpsc::Sender<CoreCommandResult>,
    core: &Arc<StereodromeCore>,
    tokio_runtime: &tokio::runtime::Runtime,
    completion_mailbox: &SyncSender<MailboxMessage>,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_operation_id: &mut u64,
    next_event_id: &mut u64,
    pending_effects: &mut HashMap<OperationId, PendingEffect>,
    result_cache: &mut ResultCache,
) {
    let mut detached_value = Value::Null;
    let mut task_command = request.command.clone();
    if let CoreCommand::SetPlaylistSavedOffline {
        playlist_id,
        saved_offline,
    } = &request.command
    {
        let marked = match core.mark_playlist_saved_offline(playlist_id.clone(), *saved_offline) {
            Ok(marked) => marked,
            Err(error) => {
                let result = CoreCommandResult::failed(
                    request.command_id,
                    state.revision,
                    None,
                    ProtocolError::from(&error),
                );
                result_cache.insert(&request.command, result.clone());
                let _ = response.send(result);
                return;
            }
        };
        detached_value = match serde_json::to_value(marked) {
            Ok(value) => value,
            Err(error) => {
                let error = CoreError::from(error);
                let result = CoreCommandResult::failed(
                    request.command_id,
                    state.revision,
                    None,
                    ProtocolError::from(&error),
                );
                result_cache.insert(&request.command, result.clone());
                let _ = response.send(result);
                return;
            }
        };
        state.library_revision = state.library_revision.wrapping_add(1);
        if !*saved_offline {
            state.revision = state.revision.wrapping_add(1);
            let result = CoreCommandResult::succeeded(
                request.command_id,
                state.revision,
                None,
                detached_value,
            );
            emit_snapshot_event(
                core,
                events,
                stream_id,
                next_event_id,
                state,
                request.command_id,
                None,
            );
            result_cache.insert(&request.command, result.clone());
            let _ = response.send(result);
            return;
        }
        task_command = CoreCommand::DownloadPlaylist {
            playlist_id: playlist_id.clone(),
        };
    }

    let mut kind = request.command.job_kind();
    if matches!(kind, JobKind::BackgroundTick)
        && let Ok(Some(due)) = core.next_due_library_sync_job()
    {
        kind = JobKind::Sync {
            kind: match due {
                crate::DueSyncJob::Incremental => crate::SyncKind::Incremental,
                crate::DueSyncJob::FullReconcile => crate::SyncKind::FullReconcile,
            },
        };
    }
    let duplicate = pending_effects.iter().find_map(|(id, pending)| {
        let pending_kind = pending.command.job_kind();
        let conflicts = matches!(
            (&kind, &pending_kind),
            (
                JobKind::Sync { .. } | JobKind::BackgroundTick,
                JobKind::Sync { .. } | JobKind::BackgroundTick
            ) | (
                JobKind::SavedPlaylistReconcile,
                JobKind::SavedPlaylistReconcile
            ) | (JobKind::QueuePrefetch, JobKind::QueuePrefetch)
        );
        conflicts.then_some(*id)
    });
    if let Some(operation_id) = duplicate {
        let result = if request.command.is_detached_effect()
            && !matches!(kind, JobKind::Sync { .. } | JobKind::BackgroundTick)
        {
            CoreCommandResult::succeeded(
                request.command_id,
                state.revision,
                Some(operation_id),
                detached_value,
            )
        } else {
            CoreCommandResult::failed(
                request.command_id,
                state.revision,
                Some(operation_id),
                ProtocolError::new(
                    ProtocolErrorCode::Conflict,
                    "a conflicting runtime operation is already running",
                    true,
                ),
            )
        };
        result_cache.insert(&request.command, result.clone());
        let _ = response.send(result);
        return;
    }

    let operation_id = OperationId(*next_operation_id);
    *next_operation_id = next_operation_id.wrapping_add(1);
    let detached = request.command.is_detached_effect();
    let operation = OperationSnapshot {
        operation_id,
        cause_command_id: request.command_id,
        kind,
        phase: OperationPhase::Running,
    };
    state.operations.insert(operation_id, operation);
    if matches!(
        request.command,
        CoreCommand::ReconcileSavedPlaylistsOffline
            | CoreCommand::StartSavedPlaylistsOfflineReconcile
            | CoreCommand::SetPlaylistSavedOffline {
                saved_offline: true,
                ..
            }
    ) {
        state.saved_playlist_offline.running = true;
        state.saved_playlist_offline.operation_id = Some(operation_id);
        state.saved_playlist_offline.last_error = None;
    }
    state.revision = state.revision.wrapping_add(1);
    emit_snapshot_event(
        core,
        events,
        stream_id,
        next_event_id,
        state,
        request.command_id,
        Some(operation_id),
    );

    let cancellation = CancellationToken::new();
    let effect_cancellation = cancellation.clone();
    let effect_core = Arc::clone(core);
    let effect_command = task_command;
    let sender = completion_mailbox.clone();
    let task = tokio_runtime.spawn(async move {
        let result = run_effect(&effect_core, effect_command, &effect_cancellation).await;
        let _ = sender.send(MailboxMessage::EffectCompleted {
            operation_id,
            result,
        });
    });
    let pending = PendingEffect {
        command_id: request.command_id,
        command: request.command.clone(),
        response: (!detached).then_some(response.clone()),
        cancellation,
        abort_handle: task.abort_handle(),
    };
    pending_effects.insert(operation_id, pending);

    if detached {
        let result = CoreCommandResult::succeeded(
            request.command_id,
            state.revision,
            Some(operation_id),
            detached_value,
        );
        result_cache.insert(&request.command, result.clone());
        let _ = response.send(result);
    }
}

async fn run_effect(
    core: &StereodromeCore,
    command: CoreCommand,
    cancellation: &CancellationToken,
) -> CoreResult<Value> {
    match command {
        CoreCommand::RunBackgroundTick => {
            if core.manual_offline_enabled()? {
                return Ok(Value::Null);
            }
            let status = core.restore_session().await?;
            if !status.connected {
                return Ok(Value::Null);
            }
            serde_json::to_value(core.run_due_library_sync().await?).map_err(CoreError::from)
        }
        CoreCommand::ReportNetwork { available: true } => {
            serde_json::to_value(core.restore_session().await?).map_err(CoreError::from)
        }
        CoreCommand::StartSavedPlaylistsOfflineReconcile => {
            serde_json::to_value(core.reconcile_saved_playlists_offline().await?)
                .map_err(CoreError::from)
        }
        CoreCommand::StartQueuePrefetch { .. } => {
            let settings = core.get_audio_processing_settings()?;
            let plan = core.queue_prefetch_plan(settings.prefetch_count as usize)?;
            let outcome = core.run_queue_prefetch_plan(&plan, cancellation).await?;
            serde_json::to_value(outcome.statuses).map_err(CoreError::from)
        }
        command => effect::execute(core, command).await,
    }
}

#[allow(clippy::too_many_arguments)]
fn complete_effect(
    operation_id: OperationId,
    result: CoreResult<Value>,
    core: &StereodromeCore,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_event_id: &mut u64,
    pending_effects: &mut HashMap<OperationId, PendingEffect>,
    result_cache: &mut ResultCache,
) {
    let Some(pending) = pending_effects.remove(&operation_id) else {
        return;
    };
    state.operations.remove(&operation_id);
    state.revision = state.revision.wrapping_add(1);
    let changes_settings = pending.command.changes_settings();
    let changes_library = pending.command.changes_library();

    let command_result = match result {
        Ok(value) => {
            state.last_failure = None;
            if changes_settings {
                state.settings_revision = state.settings_revision.wrapping_add(1);
            }
            if changes_library
                && !matches!(pending.command, CoreCommand::SetPlaylistSavedOffline { .. })
                && !value.is_null()
            {
                state.library_revision = state.library_revision.wrapping_add(1);
            }
            update_connectivity(state, core, &pending.command, &value);
            if state.saved_playlist_offline.operation_id == Some(operation_id) {
                state.saved_playlist_offline.running = false;
                state.saved_playlist_offline.operation_id = None;
                state.saved_playlist_offline.last_error = None;
            }
            emit_snapshot_event(
                core,
                events,
                stream_id,
                next_event_id,
                state,
                pending.command_id,
                Some(operation_id),
            );
            CoreCommandResult::succeeded(
                pending.command_id,
                state.revision,
                Some(operation_id),
                value,
            )
        }
        Err(error) => {
            let protocol_error = ProtocolError::from(&error);
            let failure = OperationFailure {
                command_id: pending.command_id,
                operation_id: Some(operation_id),
                error: protocol_error.clone(),
            };
            state.last_failure = Some(failure.clone());
            if state.saved_playlist_offline.operation_id == Some(operation_id) {
                state.saved_playlist_offline.running = false;
                state.saved_playlist_offline.operation_id = None;
                state.saved_playlist_offline.last_error = Some(protocol_error.message.clone());
            }
            emit_event(
                events,
                stream_id,
                next_event_id,
                state.revision,
                pending.command_id,
                Some(operation_id),
                CoreEventKind::OperationFailed { failure },
            );
            emit_snapshot_event(
                core,
                events,
                stream_id,
                next_event_id,
                state,
                pending.command_id,
                Some(operation_id),
            );
            CoreCommandResult::failed(
                pending.command_id,
                state.revision,
                Some(operation_id),
                protocol_error,
            )
        }
    };

    if let Some(response) = pending.response {
        result_cache.insert(&pending.command, command_result.clone());
        let _ = response.send(command_result);
    }
}

#[allow(clippy::too_many_arguments)]
fn cancel_operation(
    cancel_command_id: CommandId,
    operation_id: OperationId,
    core: &StereodromeCore,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_event_id: &mut u64,
    pending_effects: &mut HashMap<OperationId, PendingEffect>,
    result_cache: &mut ResultCache,
) -> CoreCommandResult {
    let cancelled = cancel_pending_effect(
        operation_id,
        core,
        events,
        stream_id,
        state,
        next_event_id,
        pending_effects,
        result_cache,
    );
    if cancelled {
        CoreCommandResult::succeeded(cancel_command_id, state.revision, None, Value::Null)
    } else {
        CoreCommandResult::failed(
            cancel_command_id,
            state.revision,
            None,
            ProtocolError::new(
                ProtocolErrorCode::InvalidInput,
                format!("operation {} is not running", operation_id.0),
                false,
            ),
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn cancel_pending_effect(
    operation_id: OperationId,
    core: &StereodromeCore,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_event_id: &mut u64,
    pending_effects: &mut HashMap<OperationId, PendingEffect>,
    result_cache: &mut ResultCache,
) -> bool {
    let Some(pending) = pending_effects.remove(&operation_id) else {
        return false;
    };
    pending.cancellation.cancel();
    pending.abort_handle.abort();
    state.operations.remove(&operation_id);
    state.revision = state.revision.wrapping_add(1);
    if state.saved_playlist_offline.operation_id == Some(operation_id) {
        state.saved_playlist_offline.running = false;
        state.saved_playlist_offline.operation_id = None;
        state.saved_playlist_offline.last_error = Some("operation cancelled".to_string());
    }
    let error = ProtocolError::new(ProtocolErrorCode::Cancelled, "operation cancelled", false);
    let failure = OperationFailure {
        command_id: pending.command_id,
        operation_id: Some(operation_id),
        error: error.clone(),
    };
    state.last_failure = Some(failure.clone());
    emit_event(
        events,
        stream_id,
        next_event_id,
        state.revision,
        pending.command_id,
        Some(operation_id),
        CoreEventKind::OperationFailed { failure },
    );
    emit_snapshot_event(
        core,
        events,
        stream_id,
        next_event_id,
        state,
        pending.command_id,
        Some(operation_id),
    );
    if let Some(response) = pending.response {
        let result = CoreCommandResult::failed(
            pending.command_id,
            state.revision,
            Some(operation_id),
            error,
        );
        result_cache.insert(&pending.command, result.clone());
        let _ = response.send(result);
    }
    true
}

fn cancel_all_effects(pending_effects: &mut HashMap<OperationId, PendingEffect>, revision: u64) {
    for (operation_id, pending) in pending_effects.drain() {
        pending.cancellation.cancel();
        pending.abort_handle.abort();
        if let Some(response) = pending.response {
            let _ = response.send(CoreCommandResult::failed(
                pending.command_id,
                revision,
                Some(operation_id),
                ProtocolError::new(
                    ProtocolErrorCode::Cancelled,
                    "runtime is shutting down",
                    false,
                ),
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_snapshot_event(
    core: &StereodromeCore,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    next_event_id: &mut u64,
    state: &CoreState,
    command_id: CommandId,
    operation_id: Option<OperationId>,
) {
    if let Ok(snapshot) = build_snapshot(core, state) {
        emit_event(
            events,
            stream_id,
            next_event_id,
            state.revision,
            command_id,
            operation_id,
            CoreEventKind::SnapshotChanged {
                snapshot: Box::new(snapshot),
            },
        );
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

    if matches!(
        request.command,
        CoreCommand::ExportPortableBackup { .. } | CoreCommand::ImportPortableBackup { .. }
    ) && state.operations.values().any(|operation| {
        matches!(
            operation.kind,
            JobKind::Sync { .. }
                | JobKind::BackgroundTick
                | JobKind::DownloadSong { .. }
                | JobKind::DownloadAlbum { .. }
                | JobKind::DownloadPlaylist { .. }
                | JobKind::SavedPlaylistReconcile
                | JobKind::QueuePrefetch
        )
    }) {
        return CoreCommandResult::failed(
            request.command_id,
            state.revision,
            None,
            ProtocolError::new(
                ProtocolErrorCode::Conflict,
                "wait for background library or download jobs to finish before using backups",
                true,
            ),
        );
    }

    if let CoreCommand::ReportLifecycle { lifecycle } = &request.command {
        state.platform_lifecycle = *lifecycle;
        state.revision = state.revision.wrapping_add(1);
        emit_snapshot_event(
            core,
            events,
            stream_id,
            next_event_id,
            state,
            request.command_id,
            None,
        );
        return CoreCommandResult::succeeded(request.command_id, state.revision, None, Value::Null);
    }
    if let CoreCommand::ReportNetwork { available: false } = &request.command {
        state.network_available = false;
        tokio_runtime.block_on(core.deactivate_session());
        state.connectivity = match &state.connectivity {
            ConnectivityState::Online {
                server_url,
                username,
                ..
            } => ConnectivityState::Disconnected {
                server_url: server_url.clone(),
                username: username.clone(),
            },
            connectivity => connectivity.clone(),
        };
        state.revision = state.revision.wrapping_add(1);
        emit_snapshot_event(
            core,
            events,
            stream_id,
            next_event_id,
            state,
            request.command_id,
            None,
        );
        return CoreCommandResult::succeeded(request.command_id, state.revision, None, Value::Null);
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

    if matches!(
        request.command,
        CoreCommand::GetConnectionStatus
            | CoreCommand::GetLibrarySyncStatus
            | CoreCommand::GetSavedPlaylistsOfflineStatus
    ) {
        return match build_snapshot(core, state) {
            Ok(snapshot) => {
                let value = match &request.command {
                    CoreCommand::GetConnectionStatus => connectivity_status(&snapshot.connectivity),
                    CoreCommand::GetLibrarySyncStatus => {
                        serde_json::to_value(snapshot.sync).map_err(CoreError::from)
                    }
                    CoreCommand::GetSavedPlaylistsOfflineStatus => {
                        serde_json::to_value(snapshot.saved_playlist_offline)
                            .map_err(CoreError::from)
                    }
                    _ => unreachable!(),
                };
                match value {
                    Ok(value) => CoreCommandResult::succeeded(
                        request.command_id,
                        state.revision,
                        None,
                        value,
                    ),
                    Err(error) => CoreCommandResult::failed(
                        request.command_id,
                        state.revision,
                        None,
                        ProtocolError::from(&error),
                    ),
                }
            }
            Err(error) => CoreCommandResult::failed(
                request.command_id,
                state.revision,
                None,
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
        | CoreCommand::RestoreSession
        | CoreCommand::ReportNetwork { available: true } => {
            if matches!(command, CoreCommand::ReportNetwork { available: true }) {
                state.network_available = true;
            }
            if core.manual_offline_enabled().unwrap_or(false) {
                if let Ok(connectivity) = initial_connectivity(core) {
                    state.connectivity = connectivity;
                }
            } else if let Ok(status) = serde_json::from_value::<ConnectionStatus>(value.clone()) {
                if status.connected {
                    state.network_available = true;
                }
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

fn connectivity_status(connectivity: &ConnectivityState) -> CoreResult<Value> {
    let status = match connectivity {
        ConnectivityState::Unconfigured => ConnectionStatus::disconnected(),
        ConnectivityState::OfflineManual {
            server_url,
            username,
        } => ConnectionStatus {
            connected: false,
            server_url: server_url.clone(),
            username: username.clone(),
            server_version: None,
        },
        ConnectivityState::Disconnected {
            server_url,
            username,
        } => ConnectionStatus {
            connected: false,
            server_url: Some(server_url.clone()),
            username: Some(username.clone()),
            server_version: None,
        },
        ConnectivityState::Online {
            server_url,
            username,
            server_version,
        } => ConnectionStatus {
            connected: true,
            server_url: Some(server_url.clone()),
            username: Some(username.clone()),
            server_version: server_version.clone(),
        },
    };
    serde_json::to_value(status).map_err(CoreError::from)
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
    use std::time::{Duration, Instant};

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

    fn next_event(events: &mut broadcast::Receiver<CoreEvent>, timeout: Duration) -> CoreEvent {
        let deadline = Instant::now() + timeout;
        loop {
            match events.try_recv() {
                Ok(event) => return event,
                Err(broadcast::error::TryRecvError::Empty) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("runtime event unavailable: {error}"),
            }
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

    #[test]
    fn sync_registry_is_authoritative_in_snapshot() {
        let data_dir = test_dir("sync-registry");
        let handle = StereodromeRuntimeHandle::start(&data_dir).expect("runtime starts");
        let mut events = handle.subscribe();
        let started = handle.dispatch_command(CoreCommand::StartSync {
            kind: crate::SyncKind::FullReconcile,
        });
        assert_eq!(started.status, CommandStatus::Succeeded);
        assert!(started.operation_id.is_some());

        let event = next_event(&mut events, Duration::from_secs(1));
        let CoreEventKind::SnapshotChanged { snapshot } = event.kind else {
            panic!("sync start emits a snapshot");
        };
        assert_eq!(snapshot.sync.active_job.as_deref(), Some("full_reconcile"));
        assert!(snapshot.sync.full_reconcile.running);
        assert_eq!(snapshot.operations.len(), 1);

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let event = next_event(&mut events, Duration::from_secs(1));
            if let CoreEventKind::SnapshotChanged { snapshot } = event.kind
                && snapshot.sync.active_job.is_none()
            {
                assert!(snapshot.operations.is_empty());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "sync completion repairs snapshot"
            );
        }

        handle.shutdown();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn connectivity_and_platform_inputs_are_authoritative() {
        let data_dir = test_dir("connectivity-state");
        let handle = StereodromeRuntimeHandle::start(&data_dir).expect("runtime starts");
        let offline = handle.dispatch_command(CoreCommand::SetConnectivity {
            settings: crate::ConnectivitySettings {
                manual_offline_enabled: true,
            },
        });
        assert_eq!(offline.status, CommandStatus::Succeeded);
        let network = handle.dispatch_command(CoreCommand::ReportNetwork { available: false });
        assert_eq!(network.status, CommandStatus::Succeeded);
        let lifecycle = handle.dispatch_command(CoreCommand::ReportLifecycle {
            lifecycle: crate::PlatformLifecycle::Background,
        });
        assert_eq!(lifecycle.status, CommandStatus::Succeeded);

        let snapshot = handle.snapshot().value.expect("snapshot value");
        assert_eq!(snapshot["connectivity"]["status"], "offline-manual");
        assert_eq!(snapshot["network_available"], false);
        assert_eq!(snapshot["platform_lifecycle"], "background");
        handle.shutdown();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn prefetch_cancellation_and_backup_exclusion_are_runtime_invariants() {
        let data_dir = test_dir("prefetch-registry");
        let handle = StereodromeRuntimeHandle::start(&data_dir).expect("runtime starts");
        let _ = handle.dispatch_command(CoreCommand::AddSongsToQueue {
            items: vec![queue_item(1), queue_item(2)],
        });
        let _ = handle.dispatch_command(CoreCommand::PlayQueueItem { index: 0 });

        let started = handle.dispatch_command(CoreCommand::StartQueuePrefetch {
            reserve_first: false,
        });
        let operation_id = started.operation_id.expect("prefetch has operation ID");
        let backup = handle.dispatch_command(CoreCommand::ExportPortableBackup {
            path: data_dir.join("blocked.zip").to_string_lossy().into_owned(),
        });
        assert_eq!(backup.status, CommandStatus::Failed);
        assert!(matches!(
            backup.error,
            Some(ProtocolError {
                code: ProtocolErrorCode::Conflict,
                ..
            })
        ));

        let cancelled = handle.dispatch_command(CoreCommand::CancelOperation { operation_id });
        assert_eq!(cancelled.status, CommandStatus::Succeeded);
        let snapshot = handle.snapshot().value.expect("snapshot value");
        assert!(snapshot["operations"].as_array().unwrap().is_empty());
        handle.shutdown();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn shutdown_cancels_owned_effects_with_a_bound() {
        let data_dir = test_dir("bounded-shutdown");
        let handle = StereodromeRuntimeHandle::start(&data_dir).expect("runtime starts");
        let _ = handle.dispatch_command(CoreCommand::AddSongsToQueue {
            items: vec![queue_item(1), queue_item(2)],
        });
        let _ = handle.dispatch_command(CoreCommand::PlayQueueItem { index: 0 });
        let _ = handle.dispatch_command(CoreCommand::StartQueuePrefetch {
            reserve_first: false,
        });

        let started = Instant::now();
        handle.shutdown();
        assert!(started.elapsed() < Duration::from_secs(3));
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
