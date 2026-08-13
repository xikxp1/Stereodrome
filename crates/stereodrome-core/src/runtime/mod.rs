//! Serialized runtime shell around the existing core services.

mod effect;
mod playback;
mod snapshot;
mod state;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use stereodrome_audio::AudioNotification;
use tokio::sync::broadcast;
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    CORE_PROTOCOL_VERSION, CommandId, ConnectivityState, CoreCommand, CoreCommandRequest,
    CoreCommandResult, CoreEvent, CoreEventKind, JobKind, OperationFailure, OperationId,
    OperationPhase, OperationSnapshot, ProtocolError, ProtocolErrorCode, RuntimeLifecycle,
};
use crate::{ConnectionStatus, CoreError, CoreResult, StereodromeCore};

pub use self::playback::{AudioPort, PlaybackClock, PreparedAudio, StereodromeAudioPort};
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
    PlaybackPrepared {
        operation_id: OperationId,
        result: CoreResult<playback::PreparedPlayback>,
    },
    PlaybackNotification(AudioNotification),
    PlaybackTick,
    Stop,
}

struct PendingEffect {
    command_id: CommandId,
    command: CoreCommand,
    response: Option<mpsc::Sender<CoreCommandResult>>,
    cancellation: CancellationToken,
    abort_handle: AbortHandle,
}

struct PendingPlayback {
    command_id: CommandId,
    command: CoreCommand,
    response: Option<mpsc::Sender<CoreCommandResult>>,
    cancellation: CancellationToken,
    abort_handle: AbortHandle,
    success_value: Option<Value>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CancellationReporting {
    /// The runtime superseded obsolete work as part of another command.
    Silent,
    /// A caller explicitly cancelled an operation and should observe its terminal state.
    Failure,
}

struct RuntimeInner {
    mailbox: SyncSender<MailboxMessage>,
    events: broadcast::Sender<CoreEvent>,
    next_command_id: AtomicU64,
    stopped: AtomicBool,
    monitor_running: Arc<AtomicBool>,
    tick_gate: Arc<PlaybackTickGate>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Drop for RuntimeInner {
    fn drop(&mut self) {
        if !self.stopped.swap(true, Ordering::SeqCst) {
            let _ = self.mailbox.send(MailboxMessage::Stop);
        }
        self.monitor_running.store(false, Ordering::SeqCst);
        self.tick_gate.stop();
        if let Ok(thread) = self.thread.get_mut()
            && let Some(thread) = thread.take()
        {
            let _ = thread.join();
        }
    }
}

/// Blocks the playback tick thread while nothing is playing so a paused or
/// idle runtime schedules zero periodic wakeups.
struct PlaybackTickGate {
    state: Mutex<TickGateState>,
    condvar: Condvar,
}

struct TickGateState {
    playing: bool,
    running: bool,
}

impl PlaybackTickGate {
    fn new() -> Self {
        Self {
            state: Mutex::new(TickGateState {
                playing: false,
                running: true,
            }),
            condvar: Condvar::new(),
        }
    }

    fn set_playing(&self, playing: bool) {
        if let Ok(mut state) = self.state.lock()
            && state.playing != playing
        {
            state.playing = playing;
            self.condvar.notify_all();
        }
    }

    fn stop(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.running = false;
            self.condvar.notify_all();
        }
    }

    /// Blocks until playback is active; returns false once the runtime stops.
    fn wait_until_playing(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        while state.running && !state.playing {
            state = match self.condvar.wait(state) {
                Ok(state) => state,
                Err(_) => return false,
            };
        }
        state.running
    }

    fn is_running(&self) -> bool {
        self.state.lock().is_ok_and(|state| state.running)
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
        let audio: Arc<dyn AudioPort> = Arc::new(StereodromeAudioPort::new()?);
        Self::start_with_core_and_audio(data_dir, core, audio)
    }

    /// Starts a runtime with an injected audio port for deterministic tests.
    ///
    /// # Errors
    /// Returns an error if a runtime already owns the directory or the actor
    /// thread/runtime cannot start.
    pub fn start_with_core_and_audio(
        data_dir: impl AsRef<Path>,
        core: Arc<StereodromeCore>,
        audio: Arc<dyn AudioPort>,
    ) -> CoreResult<Self> {
        Self::start_with_core_audio_and_clock(
            data_dir,
            core,
            audio,
            Arc::new(playback::SystemPlaybackClock),
        )
    }

    /// Starts a runtime with injected audio and clock boundaries.
    ///
    /// # Errors
    /// Returns an error if a runtime already owns the directory or the actor
    /// thread/runtime cannot start.
    pub fn start_with_core_audio_and_clock(
        data_dir: impl AsRef<Path>,
        core: Arc<StereodromeCore>,
        audio: Arc<dyn AudioPort>,
        clock: Arc<dyn PlaybackClock>,
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
        let monitor_running = Arc::new(AtomicBool::new(true));
        let tick_gate = Arc::new(PlaybackTickGate::new());
        start_playback_inputs(
            &audio,
            mailbox.clone(),
            Arc::clone(&monitor_running),
            Arc::clone(&tick_gate),
        );
        let actor_tick_gate = Arc::clone(&tick_gate);
        let actor_thread = thread::Builder::new()
            .name("stereodrome-runtime".to_string())
            .spawn(move || {
                run_actor(
                    receiver,
                    completion_mailbox,
                    actor_events,
                    stream_id,
                    core,
                    audio,
                    clock,
                    connectivity,
                    tokio_runtime,
                    actor_tick_gate,
                    lease,
                );
            })?;
        Ok(Self {
            inner: Arc::new(RuntimeInner {
                mailbox,
                events,
                next_command_id: AtomicU64::new(GENERATED_COMMAND_ID_START),
                stopped: AtomicBool::new(false),
                monitor_running,
                tick_gate,
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
        self.inner.monitor_running.store(false, Ordering::SeqCst);
        self.inner.tick_gate.stop();
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

fn start_playback_inputs(
    audio: &Arc<dyn AudioPort>,
    mailbox: SyncSender<MailboxMessage>,
    running: Arc<AtomicBool>,
    tick_gate: Arc<PlaybackTickGate>,
) {
    if let Some(notifications) = audio.take_notifications() {
        let notification_mailbox = mailbox.clone();
        let notification_running = running;
        // Blocks until the audio engine sends a notification; the channel
        // disconnects when the audio port is dropped at shutdown, so no
        // periodic wakeup is needed to observe the running flag.
        thread::spawn(move || {
            while notification_running.load(Ordering::SeqCst) {
                let Ok(notification) = notifications.recv() else {
                    break;
                };
                if notification_mailbox
                    .send(MailboxMessage::PlaybackNotification(notification))
                    .is_err()
                {
                    break;
                }
            }
        });
    }
    thread::spawn(move || {
        while tick_gate.wait_until_playing() {
            thread::sleep(Duration::from_millis(250));
            if !tick_gate.is_running() || mailbox.send(MailboxMessage::PlaybackTick).is_err() {
                break;
            }
        }
    });
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
    audio: Arc<dyn AudioPort>,
    clock: Arc<dyn PlaybackClock>,
    connectivity: ConnectivityState,
    tokio_runtime: tokio::runtime::Runtime,
    tick_gate: Arc<PlaybackTickGate>,
    _lease: RuntimeLease,
) {
    let mut state = CoreState::new(connectivity);
    state.lifecycle = RuntimeLifecycle::Ready;
    let mut next_operation_id = 1_u64;
    let mut next_event_id = 1_u64;
    let mut next_internal_command_id = GENERATED_COMMAND_ID_START + (1 << 62);
    let mut result_cache = ResultCache::new();
    let mut pending_effects = HashMap::<OperationId, PendingEffect>::new();
    let mut pending_playback = HashMap::<OperationId, PendingPlayback>::new();
    let mut last_playback_projection = playback::projection(&core, audio.as_ref(), None).ok();
    let mut last_tick_fingerprint: Option<PlaybackTickFingerprint> = None;
    let mut last_progress_at = clock.now();
    let mut last_segment_index = 0_usize;

    while let Ok(message) = receiver.recv() {
        // Every playback transition produces at least one mailbox message
        // (commands directly, engine transitions via PlaybackChanged), so
        // sampling here keeps the gate current without its own timer.
        tick_gate.set_playing(audio.status().is_playing);
        let MailboxMessage::Dispatch { request, response } = message else {
            match message {
                MailboxMessage::EffectCompleted {
                    operation_id,
                    result,
                } => complete_effect(
                    operation_id,
                    result,
                    &core,
                    audio.as_ref(),
                    &events,
                    stream_id,
                    &mut state,
                    &mut next_event_id,
                    &mut pending_effects,
                    &mut result_cache,
                ),
                MailboxMessage::PlaybackPrepared {
                    operation_id,
                    result,
                } => complete_playback(
                    operation_id,
                    result,
                    &core,
                    audio.as_ref(),
                    &events,
                    stream_id,
                    &mut state,
                    &mut next_event_id,
                    &completion_mailbox,
                    &mut next_internal_command_id,
                    &mut pending_playback,
                    &mut result_cache,
                ),
                MailboxMessage::PlaybackNotification(notification) => handle_playback_input(
                    Some(notification),
                    &core,
                    audio.as_ref(),
                    &tokio_runtime,
                    &completion_mailbox,
                    &events,
                    stream_id,
                    &mut state,
                    &mut next_operation_id,
                    &mut next_event_id,
                    &mut pending_playback,
                    &mut last_playback_projection,
                    &mut last_tick_fingerprint,
                    clock.as_ref(),
                    &mut last_progress_at,
                    &mut last_segment_index,
                ),
                MailboxMessage::PlaybackTick => handle_playback_input(
                    None,
                    &core,
                    audio.as_ref(),
                    &tokio_runtime,
                    &completion_mailbox,
                    &events,
                    stream_id,
                    &mut state,
                    &mut next_operation_id,
                    &mut next_event_id,
                    &mut pending_playback,
                    &mut last_playback_projection,
                    &mut last_tick_fingerprint,
                    clock.as_ref(),
                    &mut last_progress_at,
                    &mut last_segment_index,
                ),
                MailboxMessage::Stop => break,
                MailboxMessage::Dispatch { .. } => unreachable!(),
            }
            continue;
        };
        if request.protocol_version != CORE_PROTOCOL_VERSION || request.command_id.0 == 0 {
            let error = if request.protocol_version == CORE_PROTOCOL_VERSION {
                ProtocolError::new(
                    ProtocolErrorCode::InvalidCommandId,
                    "command_id must be greater than zero",
                    false,
                )
            } else {
                ProtocolError::new(
                    ProtocolErrorCode::UnsupportedProtocolVersion,
                    format!(
                        "unsupported protocol version {}; expected {CORE_PROTOCOL_VERSION}",
                        request.protocol_version
                    ),
                    false,
                )
            };
            let result = CoreCommandResult::failed(request.command_id, state.revision, None, error);
            let _ = response.send(result);
            continue;
        }
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
        if request.command.invalidates_queue_prefetch()
            && let Some(operation_id) = pending_effects.iter().find_map(|(id, pending)| {
                matches!(pending.command, CoreCommand::StartQueuePrefetch { .. }).then_some(*id)
            })
        {
            let _ = cancel_pending_effect(
                operation_id,
                &core,
                audio.as_ref(),
                &events,
                stream_id,
                &mut state,
                &mut next_event_id,
                &mut pending_effects,
                &mut result_cache,
                CancellationReporting::Silent,
            );
        }
        if let CoreCommand::CancelOperation { operation_id } = &request.command {
            if cancel_pending_playback(
                *operation_id,
                &core,
                audio.as_ref(),
                &events,
                stream_id,
                &mut state,
                &mut next_event_id,
                &mut pending_playback,
                &mut result_cache,
                CancellationReporting::Failure,
            ) {
                let result = CoreCommandResult::succeeded(
                    request.command_id,
                    state.revision,
                    None,
                    Value::Null,
                );
                result_cache.insert(&request.command, result.clone());
                let _ = response.send(result);
                continue;
            }
            let result = cancel_operation(
                request.command_id,
                *operation_id,
                &core,
                audio.as_ref(),
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
        if is_playback_command(&request.command)
            && request.protocol_version == CORE_PROTOCOL_VERSION
            && request.command_id.0 != 0
        {
            process_playback_request(
                request,
                response,
                &core,
                audio.as_ref(),
                &tokio_runtime,
                &completion_mailbox,
                &events,
                stream_id,
                &mut state,
                &mut next_operation_id,
                &mut next_event_id,
                &mut pending_playback,
                &mut result_cache,
            );
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
                    audio.as_ref(),
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
                audio.as_ref(),
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
                audio.as_ref(),
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
            audio.as_ref(),
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
    cancel_all_playback(&mut pending_playback, state.revision);
    let _ = audio.stop();
    tokio_runtime.shutdown_timeout(Duration::from_secs(2));
}

fn is_playback_command(command: &CoreCommand) -> bool {
    matches!(
        command,
        CoreCommand::PlaySelection { .. }
            | CoreCommand::ClearPlayback
            | CoreCommand::NavigatePlayback { .. }
            | CoreCommand::TogglePlayback
            | CoreCommand::PausePlayback
            | CoreCommand::ResumePlayback
            | CoreCommand::StopPlayback
            | CoreCommand::SeekTo { .. }
            | CoreCommand::SeekBy { .. }
            | CoreCommand::SetPlaybackVolume { .. }
            | CoreCommand::RebuildAudioOutput
            | CoreCommand::ApplyAudioSettings
            | CoreCommand::PrepareNextTransition
            | CoreCommand::ReportPlatformPlayback { .. }
            | CoreCommand::SetAudioProcessing { .. }
    )
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn process_playback_request(
    request: CoreCommandRequest,
    response: mpsc::Sender<CoreCommandResult>,
    core: &Arc<StereodromeCore>,
    audio: &dyn AudioPort,
    tokio_runtime: &tokio::runtime::Runtime,
    completion_mailbox: &SyncSender<MailboxMessage>,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_operation_id: &mut u64,
    next_event_id: &mut u64,
    pending_playback: &mut HashMap<OperationId, PendingPlayback>,
    result_cache: &mut ResultCache,
) {
    let command = request.command.clone();
    let mut prepare_request = None;
    let mut success_value = None;
    let result = match &request.command {
        CoreCommand::PlaySelection { song_id, song_ids } => audio
            .stop()
            .and_then(|()| core.play_song_with_queue(song_id.clone(), song_ids.clone()))
            .and_then(|queue| {
                let item = queue
                    .current_index
                    .and_then(|index| queue.items.get(index))
                    .cloned()
                    .ok_or_else(|| {
                        CoreError::InvalidInput("play selection is empty".to_string())
                    })?;
                prepare_request = Some((
                    item,
                    playback::PlaybackCommit::Current {
                        seek_seconds: None,
                        pause_after_start: false,
                    },
                ));
                Ok(())
            }),
        CoreCommand::NavigatePlayback { navigation } => {
            let queue = core.get_queue();
            queue.and_then(|queue| {
                let expected_current_song_id = queue
                    .current_index
                    .and_then(|index| queue.items.get(index))
                    .map(|item| item.song_id.clone());
                if let Some(item) = playback::preview_navigation(core, *navigation)? {
                    prepare_request = Some((
                        item,
                        playback::PlaybackCommit::Navigation {
                            navigation: *navigation,
                            expected_current_song_id,
                            expected_playback: audio.current_identity(),
                        },
                    ));
                    Ok(())
                } else {
                    audio.stop()?;
                    commit_empty_navigation(core, *navigation)?;
                    Ok(())
                }
            })
        }
        CoreCommand::ResumePlayback => prepare_resume(core, audio, &mut prepare_request),
        CoreCommand::TogglePlayback => {
            if audio.status().is_playing {
                audio.pause()
            } else {
                prepare_resume(core, audio, &mut prepare_request)
            }
        }
        CoreCommand::PausePlayback => audio.pause(),
        CoreCommand::StopPlayback => stop_playback(core, audio),
        CoreCommand::ClearPlayback => audio.stop().and_then(|()| core.clear_queue().map(drop)),
        CoreCommand::SeekTo { seconds } => seek_playback(core, audio, *seconds),
        CoreCommand::SeekBy { seconds } => {
            playback::projection(core, audio, state.playback_operation_id).and_then(|projection| {
                seek_playback(core, audio, projection.position_seconds + *seconds)
            })
        }
        CoreCommand::SetPlaybackVolume { volume } => audio.set_volume(*volume),
        CoreCommand::RebuildAudioOutput => audio.rebuild_output(),
        CoreCommand::ApplyAudioSettings => {
            prepare_reapply_settings(core, audio, &mut prepare_request)
        }
        CoreCommand::SetAudioProcessing { settings } => core
            .set_audio_processing_settings(settings.clone())
            .and_then(|settings| {
                success_value = Some(serde_json::to_value(settings)?);
                prepare_reapply_settings(core, audio, &mut prepare_request)
            }),
        CoreCommand::PrepareNextTransition => match playback::gapless_target(core, audio) {
            Ok(Some((item, expected_playback))) => {
                prepare_request = Some((
                    item,
                    playback::PlaybackCommit::Gapless { expected_playback },
                ));
                Ok(())
            }
            Ok(None) => {
                success_value = Some(Value::Null);
                Ok(())
            }
            Err(error) => Err(error),
        },
        CoreCommand::ReportPlatformPlayback { event } => {
            handle_platform_playback(*event, audio, state)
        }
        _ => unreachable!("checked by is_playback_command"),
    };

    if result.is_ok()
        && matches!(
            command,
            CoreCommand::PlaySelection { .. }
                | CoreCommand::NavigatePlayback { .. }
                | CoreCommand::TogglePlayback
                | CoreCommand::ResumePlayback
                | CoreCommand::StopPlayback
                | CoreCommand::ClearPlayback
        )
    {
        state.paused_by_platform = false;
    }

    if let Err(error) = result {
        finish_playback_error(
            &request,
            &response,
            &error,
            core,
            audio,
            events,
            stream_id,
            state,
            next_event_id,
            result_cache,
        );
        return;
    }

    if let Some((item, commit)) = prepare_request {
        if matches!(command, CoreCommand::SetAudioProcessing { .. }) {
            state.settings_revision = state.settings_revision.wrapping_add(1);
        }
        cancel_current_playback(
            core,
            audio,
            events,
            stream_id,
            state,
            next_event_id,
            pending_playback,
            result_cache,
        );
        start_playback_prepare(
            request,
            Some(response),
            item,
            commit,
            success_value,
            core,
            audio,
            tokio_runtime,
            completion_mailbox,
            events,
            stream_id,
            state,
            next_operation_id,
            next_event_id,
            pending_playback,
        );
        return;
    }

    if matches!(
        command,
        CoreCommand::PausePlayback
            | CoreCommand::StopPlayback
            | CoreCommand::ClearPlayback
            | CoreCommand::SeekTo { .. }
            | CoreCommand::SeekBy { .. }
            | CoreCommand::ReportPlatformPlayback { .. }
    ) {
        cancel_current_playback(
            core,
            audio,
            events,
            stream_id,
            state,
            next_event_id,
            pending_playback,
            result_cache,
        );
    }
    let _ = playback::persist_live_progress(core, audio);
    state.revision = state.revision.wrapping_add(1);
    state.last_failure = None;
    if matches!(command, CoreCommand::SetAudioProcessing { .. }) {
        state.settings_revision = state.settings_revision.wrapping_add(1);
    }
    emit_snapshot_event(
        core,
        audio,
        events,
        stream_id,
        next_event_id,
        state,
        request.command_id,
        None,
    );
    let value = success_value.unwrap_or_else(|| {
        if matches!(
            command,
            CoreCommand::NavigatePlayback { .. } | CoreCommand::ClearPlayback
        ) {
            serde_json::to_value(core.get_queue().ok()).unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    });
    let result = CoreCommandResult::succeeded(request.command_id, state.revision, None, value);
    result_cache.insert(&command, result.clone());
    let _ = response.send(result);
}

fn prepare_resume(
    core: &StereodromeCore,
    audio: &dyn AudioPort,
    prepare_request: &mut Option<(crate::QueueItem, playback::PlaybackCommit)>,
) -> CoreResult<()> {
    if audio.status().current_song_id.is_some() {
        return audio.resume();
    }
    let persisted = core.get_playback_state()?;
    let mut queue = core.get_queue()?;
    if queue.current_index.is_none()
        && let Some(saved_song_id) = persisted.current_song_id.as_deref()
        && let Some(index) = queue
            .items
            .iter()
            .position(|item| item.song_id == saved_song_id)
    {
        core.play_queue_item(index)?;
        queue = core.get_queue()?;
    }
    let Some(item) = queue
        .current_index
        .and_then(|index| queue.items.get(index))
        .cloned()
    else {
        return Ok(());
    };
    let seek_seconds = (persisted.current_song_id.as_deref() == Some(item.song_id.as_str()))
        .then_some(persisted.position_seconds.max(0.0));
    *prepare_request = Some((
        item,
        playback::PlaybackCommit::Current {
            seek_seconds,
            pause_after_start: false,
        },
    ));
    Ok(())
}

fn prepare_reapply_settings(
    core: &StereodromeCore,
    audio: &dyn AudioPort,
    prepare_request: &mut Option<(crate::QueueItem, playback::PlaybackCommit)>,
) -> CoreResult<()> {
    let status = audio.status();
    if status.current_song_id.is_none() {
        return Ok(());
    }
    let queue = core.get_queue()?;
    let item = queue
        .current_index
        .and_then(|index| queue.items.get(index))
        .cloned()
        .ok_or_else(|| CoreError::InvalidInput("active audio is not in the queue".to_string()))?;
    *prepare_request = Some((
        item,
        playback::PlaybackCommit::Current {
            seek_seconds: Some(status.position),
            pause_after_start: !status.is_playing,
        },
    ));
    Ok(())
}

fn seek_playback(core: &StereodromeCore, audio: &dyn AudioPort, seconds: f64) -> CoreResult<()> {
    if !seconds.is_finite() {
        return Err(CoreError::InvalidInput(
            "seek position must be finite".to_string(),
        ));
    }
    let position = seconds.max(0.0);
    if audio.status().current_song_id.is_some() {
        audio.seek(position)?;
        return playback::persist_live_progress(core, audio);
    }
    let persisted = core.get_playback_state()?;
    if let Some(song_id) = persisted.current_song_id {
        core.save_playback_position(crate::PlaybackProgress {
            song_id,
            position_seconds: position,
            duration_seconds: persisted.duration_seconds,
            is_playing: false,
        })?;
    }
    Ok(())
}

fn stop_playback(core: &StereodromeCore, audio: &dyn AudioPort) -> CoreResult<()> {
    let state = audio.playback_state();
    if let Some(song) = state.song {
        core.save_playback_position(crate::PlaybackProgress {
            song_id: song.id,
            position_seconds: state.position,
            duration_seconds: state.duration,
            is_playing: false,
        })?;
    }
    audio.stop()
}

fn handle_platform_playback(
    event: crate::PlatformPlaybackEvent,
    audio: &dyn AudioPort,
    state: &mut CoreState,
) -> CoreResult<()> {
    use crate::PlatformPlaybackEvent;
    match event {
        PlatformPlaybackEvent::InterruptionBegan
        | PlatformPlaybackEvent::AudioFocusLost { .. }
        | PlatformPlaybackEvent::RouteLost => {
            state.paused_by_platform = audio.status().is_playing;
            if state.paused_by_platform {
                audio.pause()?;
            }
        }
        PlatformPlaybackEvent::InterruptionEnded { should_resume } => {
            if should_resume && state.paused_by_platform {
                audio.resume()?;
            }
            state.paused_by_platform = false;
        }
        PlatformPlaybackEvent::AudioFocusGained => {
            if state.paused_by_platform {
                audio.resume()?;
                state.paused_by_platform = false;
            }
        }
        PlatformPlaybackEvent::MediaServicesReset => {
            if audio.status().current_song_id.is_some() {
                audio.rebuild_output()?;
            }
        }
    }
    Ok(())
}

fn commit_empty_navigation(
    core: &StereodromeCore,
    navigation: crate::PlaybackNavigation,
) -> CoreResult<()> {
    match navigation {
        crate::PlaybackNavigation::Index { index } => {
            core.play_queue_item(index)?;
        }
        crate::PlaybackNavigation::Next { force } => {
            core.play_next(Some(force))?;
        }
        crate::PlaybackNavigation::Previous => {
            core.play_previous()?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn start_playback_prepare(
    request: CoreCommandRequest,
    response: Option<mpsc::Sender<CoreCommandResult>>,
    item: crate::QueueItem,
    commit: playback::PlaybackCommit,
    success_value: Option<Value>,
    core: &Arc<StereodromeCore>,
    audio: &dyn AudioPort,
    tokio_runtime: &tokio::runtime::Runtime,
    completion_mailbox: &SyncSender<MailboxMessage>,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_operation_id: &mut u64,
    next_event_id: &mut u64,
    pending_playback: &mut HashMap<OperationId, PendingPlayback>,
) {
    let operation_id = OperationId(*next_operation_id);
    *next_operation_id = next_operation_id.wrapping_add(1);
    state.last_failure = None;
    state.playback_operation_id = Some(operation_id);
    state.operations.insert(
        operation_id,
        OperationSnapshot {
            operation_id,
            cause_command_id: request.command_id,
            kind: JobKind::PlaybackPrepare {
                song_id: item.song_id.clone(),
            },
            phase: OperationPhase::Running,
        },
    );
    state.revision = state.revision.wrapping_add(1);
    emit_snapshot_event(
        core,
        audio,
        events,
        stream_id,
        next_event_id,
        state,
        request.command_id,
        Some(operation_id),
    );
    let cancellation = CancellationToken::new();
    let effect_core = Arc::clone(core);
    let sender = completion_mailbox.clone();
    let task = tokio_runtime.spawn(async move {
        let result = playback::prepare(&effect_core, item, commit).await;
        let _ = sender.send(MailboxMessage::PlaybackPrepared {
            operation_id,
            result,
        });
    });
    pending_playback.insert(
        operation_id,
        PendingPlayback {
            command_id: request.command_id,
            command: request.command,
            response,
            cancellation,
            abort_handle: task.abort_handle(),
            success_value,
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn complete_playback(
    operation_id: OperationId,
    result: CoreResult<playback::PreparedPlayback>,
    core: &StereodromeCore,
    audio: &dyn AudioPort,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_event_id: &mut u64,
    completion_mailbox: &SyncSender<MailboxMessage>,
    next_internal_command_id: &mut u64,
    pending_playback: &mut HashMap<OperationId, PendingPlayback>,
    result_cache: &mut ResultCache,
) {
    let Some(pending) = pending_playback.remove(&operation_id) else {
        return;
    };
    if state.playback_operation_id != Some(operation_id) {
        return;
    }
    state.playback_operation_id = None;
    state.operations.remove(&operation_id);
    state.revision = state.revision.wrapping_add(1);
    let mut reserve_prefetch_first = false;
    let committed = result.and_then(|prepared| {
        reserve_prefetch_first =
            matches!(&prepared.commit, playback::PlaybackCommit::Gapless { .. });
        playback::commit(core, audio, prepared)
    });
    let command_result = match committed {
        Ok(value) => {
            let _ = playback::persist_live_progress(core, audio);
            state.last_failure = None;
            emit_snapshot_event(
                core,
                audio,
                events,
                stream_id,
                next_event_id,
                state,
                pending.command_id,
                Some(operation_id),
            );
            let gapless_will_prepare = !reserve_prefetch_first
                && playback::gapless_target(core, audio)
                    .ok()
                    .flatten()
                    .is_some();
            if !gapless_will_prepare {
                let (response, _) = mpsc::channel();
                let _ = completion_mailbox.try_send(MailboxMessage::Dispatch {
                    request: CoreCommandRequest {
                        protocol_version: CORE_PROTOCOL_VERSION,
                        command_id: CommandId(*next_internal_command_id),
                        command: CoreCommand::StartQueuePrefetch {
                            reserve_first: reserve_prefetch_first,
                        },
                    },
                    response,
                });
                *next_internal_command_id = next_internal_command_id.wrapping_add(1);
            }
            CoreCommandResult::succeeded(
                pending.command_id,
                state.revision,
                Some(operation_id),
                pending.success_value.unwrap_or(value),
            )
        }
        Err(error) => {
            audio.set_crossfade_initiated(false);
            let protocol_error = ProtocolError::from(&error);
            let failure = OperationFailure {
                command_id: pending.command_id,
                operation_id: Some(operation_id),
                error: protocol_error.clone(),
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
                audio,
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
fn finish_playback_error(
    request: &CoreCommandRequest,
    response: &mpsc::Sender<CoreCommandResult>,
    error: &CoreError,
    core: &StereodromeCore,
    audio: &dyn AudioPort,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_event_id: &mut u64,
    result_cache: &mut ResultCache,
) {
    state.revision = state.revision.wrapping_add(1);
    let protocol_error = ProtocolError::from(error);
    let failure = OperationFailure {
        command_id: request.command_id,
        operation_id: None,
        error: protocol_error.clone(),
    };
    state.last_failure = Some(failure.clone());
    emit_event(
        events,
        stream_id,
        next_event_id,
        state.revision,
        request.command_id,
        None,
        CoreEventKind::OperationFailed { failure },
    );
    emit_snapshot_event(
        core,
        audio,
        events,
        stream_id,
        next_event_id,
        state,
        request.command_id,
        None,
    );
    let result =
        CoreCommandResult::failed(request.command_id, state.revision, None, protocol_error);
    result_cache.insert(&request.command, result.clone());
    let _ = response.send(result);
}

#[allow(clippy::too_many_arguments)]
fn cancel_pending_playback(
    operation_id: OperationId,
    core: &StereodromeCore,
    audio: &dyn AudioPort,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_event_id: &mut u64,
    pending_playback: &mut HashMap<OperationId, PendingPlayback>,
    result_cache: &mut ResultCache,
    reporting: CancellationReporting,
) -> bool {
    let Some(pending) = pending_playback.remove(&operation_id) else {
        return false;
    };
    pending.cancellation.cancel();
    pending.abort_handle.abort();
    audio.set_crossfade_initiated(false);
    state.operations.remove(&operation_id);
    if state.playback_operation_id == Some(operation_id) {
        state.playback_operation_id = None;
    }
    state.revision = state.revision.wrapping_add(1);
    let error = ProtocolError::new(ProtocolErrorCode::Cancelled, "operation cancelled", false);
    if reporting == CancellationReporting::Failure {
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
    }
    emit_snapshot_event(
        core,
        audio,
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

#[allow(clippy::too_many_arguments)]
fn cancel_current_playback(
    core: &StereodromeCore,
    audio: &dyn AudioPort,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_event_id: &mut u64,
    pending_playback: &mut HashMap<OperationId, PendingPlayback>,
    result_cache: &mut ResultCache,
) {
    let operation_ids = pending_playback.keys().copied().collect::<Vec<_>>();
    for operation_id in operation_ids {
        let _ = cancel_pending_playback(
            operation_id,
            core,
            audio,
            events,
            stream_id,
            state,
            next_event_id,
            pending_playback,
            result_cache,
            CancellationReporting::Silent,
        );
    }
}

fn cancel_all_playback(
    pending_playback: &mut HashMap<OperationId, PendingPlayback>,
    revision: u64,
) {
    for (operation_id, pending) in pending_playback.drain() {
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

/// Cheap summary of every input that can change the playback projection short
/// of a queue mutation (covered by `queue_revision`). While it is unchanged on
/// a quiet tick, rebuilding the projection is guaranteed to be discarded by
/// `playback_projection_changed`, so the tick skips the rebuild entirely.
#[derive(Clone, PartialEq)]
struct PlaybackTickFingerprint {
    state: stereodrome_audio::PlaybackLifecycleState,
    is_playing: bool,
    song_id: Option<String>,
    output_state: stereodrome_audio::AudioOutputState,
    queue_revision: u64,
    preparing_operation_id: Option<OperationId>,
}

impl PlaybackTickFingerprint {
    fn capture(
        core: &StereodromeCore,
        audio: &dyn AudioPort,
        preparing_operation_id: Option<OperationId>,
    ) -> Self {
        let status = audio.status();
        Self {
            state: status.state,
            is_playing: status.is_playing,
            song_id: status.current_song_id,
            output_state: status.output_state,
            queue_revision: core.queue_revision(),
            preparing_operation_id,
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn handle_playback_input(
    notification: Option<AudioNotification>,
    core: &Arc<StereodromeCore>,
    audio: &dyn AudioPort,
    tokio_runtime: &tokio::runtime::Runtime,
    completion_mailbox: &SyncSender<MailboxMessage>,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_operation_id: &mut u64,
    next_event_id: &mut u64,
    pending_playback: &mut HashMap<OperationId, PendingPlayback>,
    last_projection: &mut Option<crate::PlaybackProjection>,
    last_tick_fingerprint: &mut Option<PlaybackTickFingerprint>,
    clock: &dyn PlaybackClock,
    last_progress_at: &mut std::time::Instant,
    last_segment_index: &mut usize,
) {
    let is_quiet_tick = notification.is_none();
    if notification
        .as_ref()
        .is_some_and(|notification| !audio_notification_is_current(audio, notification))
    {
        return;
    }

    let (audio_state, segment_index) = audio.gapless_state();
    if segment_index < *last_segment_index {
        *last_segment_index = 0;
    }
    if segment_index > *last_segment_index {
        *last_segment_index = segment_index;
        if let Ok(Some(item)) = core.play_next(Some(false)) {
            let progress = crate::PlaybackProgress {
                song_id: item.song_id,
                position_seconds: audio_state.position,
                duration_seconds: audio_state.duration,
                is_playing: audio_state.is_playing,
            };
            let effect_core = Arc::clone(core);
            tokio_runtime.spawn(async move {
                if let Err(error) = effect_core.report_playback_progress(progress).await {
                    log::warn!(target: "stereodrome_core", "Failed to report playback progress: {error}");
                }
            });
        }
    }

    let terminal_identity = match notification {
        Some(AudioNotification::EndOfTrack { identity }) => Some(identity),
        _ => None,
    };
    if let Some(identity) = terminal_identity
        && pending_playback.is_empty()
        && audio.current_identity().as_ref() == Some(&identity)
    {
        if let Some(song) = audio_state.song.as_ref() {
            let _ = core.save_playback_position(crate::PlaybackProgress {
                song_id: song.id.clone(),
                position_seconds: 0.0,
                duration_seconds: audio_state.duration,
                is_playing: false,
            });
        }
        start_automatic_navigation(
            core,
            audio,
            tokio_runtime,
            completion_mailbox,
            events,
            stream_id,
            state,
            next_operation_id,
            next_event_id,
            pending_playback,
            identity,
        );
    }

    let now = clock.now();
    if audio_state.is_playing
        && now.saturating_duration_since(*last_progress_at) >= Duration::from_secs(15)
    {
        *last_progress_at = now;
        let progress = audio_state
            .song
            .as_ref()
            .map(|song| crate::PlaybackProgress {
                song_id: song.id.clone(),
                position_seconds: audio_state.position,
                duration_seconds: audio_state.duration,
                is_playing: true,
            });
        if let Some(progress) = progress {
            let effect_core = Arc::clone(core);
            tokio_runtime.spawn(async move {
                if let Err(error) = effect_core.report_playback_progress(progress).await {
                    log::warn!(target: "stereodrome_core", "Failed to report playback progress: {error}");
                }
            });
        }
    }

    if pending_playback.is_empty() && audio_state.is_playing {
        if let Some((item, commit)) = crossfade_prepare(core, audio, &audio_state, segment_index) {
            start_internal_prepare(
                CoreCommand::NavigatePlayback {
                    navigation: crate::PlaybackNavigation::Next { force: false },
                },
                item,
                commit,
                core,
                audio,
                tokio_runtime,
                completion_mailbox,
                events,
                stream_id,
                state,
                next_operation_id,
                next_event_id,
                pending_playback,
            );
        } else if audio.is_last_gapless_segment(segment_index)
            && let Ok(Some((item, expected_playback))) = playback::gapless_target(core, audio)
        {
            start_internal_prepare(
                CoreCommand::PrepareNextTransition,
                item,
                playback::PlaybackCommit::Gapless { expected_playback },
                core,
                audio,
                tokio_runtime,
                completion_mailbox,
                events,
                stream_id,
                state,
                next_operation_id,
                next_event_id,
                pending_playback,
            );
        }
    }

    // Building the projection clones the queue and hits the database, so quiet
    // ticks skip it while the cheap fingerprint is unchanged; notifications and
    // any tick that mutated state above always rebuild.
    let fingerprint = PlaybackTickFingerprint::capture(core, audio, state.playback_operation_id);
    if is_quiet_tick && last_tick_fingerprint.as_ref() == Some(&fingerprint) {
        return;
    }
    *last_tick_fingerprint = Some(fingerprint);

    if let Ok(projection) = playback::projection(core, audio, state.playback_operation_id)
        && playback_projection_changed(last_projection.as_ref(), &projection)
    {
        *last_projection = Some(projection);
        state.revision = state.revision.wrapping_add(1);
        emit_snapshot_event(
            core,
            audio,
            events,
            stream_id,
            next_event_id,
            state,
            CommandId(0),
            state.playback_operation_id,
        );
    }
}

fn audio_notification_is_current(audio: &dyn AudioPort, notification: &AudioNotification) -> bool {
    let current = audio.current_identity();
    match notification {
        AudioNotification::PlaybackChanged { identity, .. } => identity == &current,
        AudioNotification::GaplessSegmentChanged { identity, .. }
        | AudioNotification::EndOfTrack { identity }
        | AudioNotification::PositionChanged { identity } => current.as_ref() == Some(identity),
        AudioNotification::OutputStateChanged { .. } => true,
    }
}

fn playback_projection_changed(
    previous: Option<&crate::PlaybackProjection>,
    next: &crate::PlaybackProjection,
) -> bool {
    previous.is_none_or(|previous| {
        previous.state != next.state
            || previous.is_playing != next.is_playing
            || previous.audio_loaded != next.audio_loaded
            || previous.output_state != next.output_state
            || previous.song.as_ref().map(|song| song.id.as_str())
                != next.song.as_ref().map(|song| song.id.as_str())
            || previous.queue_index != next.queue_index
            || previous.queue_length != next.queue_length
            || previous.preparing_operation_id != next.preparing_operation_id
    })
}

#[allow(clippy::too_many_arguments)]
fn start_internal_prepare(
    command: CoreCommand,
    item: crate::QueueItem,
    commit: playback::PlaybackCommit,
    core: &Arc<StereodromeCore>,
    audio: &dyn AudioPort,
    tokio_runtime: &tokio::runtime::Runtime,
    completion_mailbox: &SyncSender<MailboxMessage>,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_operation_id: &mut u64,
    next_event_id: &mut u64,
    pending_playback: &mut HashMap<OperationId, PendingPlayback>,
) {
    start_playback_prepare(
        CoreCommandRequest {
            protocol_version: CORE_PROTOCOL_VERSION,
            command_id: CommandId(0),
            command,
        },
        None,
        item,
        commit,
        None,
        core,
        audio,
        tokio_runtime,
        completion_mailbox,
        events,
        stream_id,
        state,
        next_operation_id,
        next_event_id,
        pending_playback,
    );
}

#[allow(clippy::too_many_arguments)]
fn start_automatic_navigation(
    core: &Arc<StereodromeCore>,
    audio: &dyn AudioPort,
    tokio_runtime: &tokio::runtime::Runtime,
    completion_mailbox: &SyncSender<MailboxMessage>,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_operation_id: &mut u64,
    next_event_id: &mut u64,
    pending_playback: &mut HashMap<OperationId, PendingPlayback>,
    expected_playback: stereodrome_audio::PlaybackIdentity,
) {
    let navigation = crate::PlaybackNavigation::Next { force: false };
    let Ok(queue) = core.get_queue() else {
        return;
    };
    let expected_current_song_id = queue
        .current_index
        .and_then(|index| queue.items.get(index))
        .map(|item| item.song_id.clone());
    match playback::preview_navigation(core, navigation) {
        Ok(Some(item)) => start_internal_prepare(
            CoreCommand::NavigatePlayback { navigation },
            item,
            playback::PlaybackCommit::Navigation {
                navigation,
                expected_current_song_id,
                expected_playback: Some(expected_playback),
            },
            core,
            audio,
            tokio_runtime,
            completion_mailbox,
            events,
            stream_id,
            state,
            next_operation_id,
            next_event_id,
            pending_playback,
        ),
        Ok(None) => {
            let _ = audio.stop();
            let _ = core.play_next(Some(false));
            let _ = playback::persist_live_progress(core, audio);
        }
        Err(error) => log::warn!(target: "stereodrome_core", "Failed to advance playback: {error}"),
    }
}

fn crossfade_prepare(
    core: &StereodromeCore,
    audio: &dyn AudioPort,
    state: &stereodrome_audio::PlaybackState,
    segment_index: usize,
) -> Option<(crate::QueueItem, playback::PlaybackCommit)> {
    if audio.is_crossfade_initiated() || !audio.is_last_gapless_segment(segment_index) {
        return None;
    }
    let settings = core.get_audio_processing_settings().ok()?;
    if !settings.crossfade_enabled {
        return None;
    }
    let remaining = state.duration - state.position;
    if remaining <= 0.5 || remaining > f64::from(settings.crossfade_duration_ms) / 1000.0 {
        return None;
    }
    let queue = core.get_queue().ok()?;
    if queue.repeat_mode == crate::RepeatMode::One {
        return None;
    }
    let current = queue
        .current_index
        .and_then(|index| queue.items.get(index))?;
    let next = core.peek_next_queue_item().ok()??;
    if settings.gapless_enabled
        && core
            .songs_are_gapless_eligible(&current.song_id, &next.song_id)
            .ok()?
    {
        return None;
    }
    let expected_playback = audio.current_identity()?;
    audio.set_crossfade_initiated(true);
    Some((
        next,
        playback::PlaybackCommit::Crossfade {
            expected_playback,
            current_song_id: current.song_id.clone(),
            duration_ms: settings.crossfade_duration_ms,
        },
    ))
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
    audio: &dyn AudioPort,
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
                audio,
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
        audio,
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
    audio: &dyn AudioPort,
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
                audio,
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
                audio,
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
    audio: &dyn AudioPort,
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
        audio,
        events,
        stream_id,
        state,
        next_event_id,
        pending_effects,
        result_cache,
        CancellationReporting::Failure,
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
    audio: &dyn AudioPort,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    state: &mut CoreState,
    next_event_id: &mut u64,
    pending_effects: &mut HashMap<OperationId, PendingEffect>,
    result_cache: &mut ResultCache,
    reporting: CancellationReporting,
) -> bool {
    let Some(pending) = pending_effects.remove(&operation_id) else {
        return false;
    };
    pending.cancellation.cancel();
    pending.abort_handle.abort();
    state.operations.remove(&operation_id);
    state.revision = state.revision.wrapping_add(1);
    if reporting == CancellationReporting::Failure
        && state.saved_playlist_offline.operation_id == Some(operation_id)
    {
        state.saved_playlist_offline.running = false;
        state.saved_playlist_offline.operation_id = None;
        state.saved_playlist_offline.last_error = Some("operation cancelled".to_string());
    }
    let error = ProtocolError::new(ProtocolErrorCode::Cancelled, "operation cancelled", false);
    if reporting == CancellationReporting::Failure {
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
    }
    emit_snapshot_event(
        core,
        audio,
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
    audio: &dyn AudioPort,
    events: &broadcast::Sender<CoreEvent>,
    stream_id: u64,
    next_event_id: &mut u64,
    state: &CoreState,
    command_id: CommandId,
    operation_id: Option<OperationId>,
) {
    if let Ok(snapshot) = build_snapshot(core, audio, state) {
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
    audio: &dyn AudioPort,
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
                | JobKind::PlaybackPrepare { .. }
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
            audio,
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
            audio,
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
        return match build_snapshot(core, audio, state) {
            Ok(snapshot) => {
                if matches!(request.command, CoreCommand::Initialize) {
                    emit_event(
                        events,
                        stream_id,
                        next_event_id,
                        state.revision,
                        request.command_id,
                        None,
                        CoreEventKind::SnapshotChanged {
                            snapshot: Box::new(snapshot.clone()),
                        },
                    );
                }
                match to_value(snapshot) {
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
                }
            }
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
        return match build_snapshot(core, audio, state) {
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

    if matches!(request.command, CoreCommand::ImportPortableBackup { .. }) {
        let _ = audio.stop();
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
                if matches!(command_for_state, CoreCommand::ImportPortableBackup { .. })
                    && let Ok(playback) = core.get_playback_state()
                {
                    #[allow(clippy::cast_possible_truncation)]
                    let volume = playback.app_volume as f32;
                    let _ = audio.set_volume(volume);
                }

                if let Ok(snapshot) = build_snapshot(core, audio, state) {
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
    use crate::queue::{QueueItem, QueueState, RepeatMode};
    use crate::test_support::{AudioCall, FakeAudio, ManualPlaybackClock};

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

    fn seed_song(core: &StereodromeCore, song_id: &str) {
        let conn = rusqlite::Connection::open(&core.db_path).expect("open test database");
        conn.execute(
            "INSERT OR IGNORE INTO artists (id, name, synced_at)
             VALUES ('artist', 'Artist', 'now')",
            [],
        )
        .expect("insert artist");
        conn.execute(
            "INSERT OR IGNORE INTO albums (id, artist_id, name, synced_at)
             VALUES ('album', 'artist', 'Album', 'now')",
            [],
        )
        .expect("insert album");
        conn.execute(
            "INSERT INTO songs
             (id, album_id, artist_id, title, disc_number, duration, synced_at)
             VALUES (?1, 'album', 'artist', 'Song', 1, 180, 'now')",
            [song_id],
        )
        .expect("insert song");
        let cache_path = core
            .audio_cache_path(song_id, crate::MOBILE_PLAYBACK_FORMAT)
            .expect("audio cache path");
        std::fs::write(cache_path, b"fake audio").expect("write cached audio");
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
    fn desktop_now_playing_query_is_a_read_only_runtime_effect() {
        let command = CoreCommand::GetNowPlaying;
        assert!(!command.is_mutation());
        assert!(command.runs_as_effect());
        let json = serde_json::to_value(command).expect("command serializes");
        assert_eq!(json["type"], "get-now-playing");
    }

    /// Guards the hand-maintained command -> payload map used by the generated
    /// `CoreCommandValue`. Desktop wrappers previously asserted `bool` and
    /// `RepeatMode` for these commands and failed to deserialize at runtime.
    #[test]
    fn queue_mutations_all_respond_with_the_projected_queue() {
        let data_dir = test_dir("queue-payloads");
        let handle = StereodromeRuntimeHandle::start(&data_dir).expect("runtime starts");
        let _ = handle.dispatch_command(CoreCommand::AddSongsToQueue {
            items: (0..3).map(queue_item).collect(),
        });

        for command in [
            CoreCommand::GetQueue,
            CoreCommand::AddToQueue {
                item: queue_item(9),
            },
            CoreCommand::InsertNext {
                item: queue_item(10),
            },
            CoreCommand::MoveQueueItem { from: 0, to: 1 },
            CoreCommand::ToggleShuffle,
            CoreCommand::SetRepeatMode {
                mode: RepeatMode::All,
            },
            CoreCommand::CycleRepeatMode,
            CoreCommand::RerollNext,
            CoreCommand::RemoveFromQueue { index: 0 },
        ] {
            let label = format!("{command:?}");
            let result = handle.dispatch_command(command);
            assert_eq!(result.status, CommandStatus::Succeeded, "{label} succeeded");
            let value = result.value.expect("payload present");
            serde_json::from_value::<QueueState>(value)
                .unwrap_or_else(|error| panic!("{label} payload is a QueueState: {error}"));
        }

        handle.shutdown();
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
    fn generated_and_caller_command_ids_use_separate_ranges() {
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

    #[test]
    fn play_selection_and_clear_are_complete_runtime_transitions() {
        let data_dir = test_dir("play-selection");
        let core = Arc::new(StereodromeCore::new(&data_dir).expect("core initializes"));
        seed_song(&core, "song-a");
        let fake = Arc::new(FakeAudio::default());
        let audio: Arc<dyn AudioPort> = fake.clone();
        let clock = Arc::new(ManualPlaybackClock::default());
        clock.advance(Duration::from_secs(1));
        let playback_clock: Arc<dyn PlaybackClock> = clock;
        let handle = StereodromeRuntimeHandle::start_with_core_audio_and_clock(
            &data_dir,
            Arc::clone(&core),
            audio,
            playback_clock,
        )
        .expect("runtime starts");

        let played = handle.dispatch_command(CoreCommand::PlaySelection {
            song_id: "song-a".to_string(),
            song_ids: vec!["song-a".to_string()],
        });
        assert_eq!(played.status, CommandStatus::Succeeded);
        assert_eq!(fake.state().song_id.as_deref(), Some("song-a"));
        let snapshot = handle.snapshot().value.expect("snapshot value");
        assert_eq!(snapshot["playback"]["song"]["id"], "song-a");
        assert_eq!(snapshot["queue"]["current_index"], 0);

        let cleared = handle.dispatch_command(CoreCommand::ClearPlayback);
        assert_eq!(cleared.status, CommandStatus::Succeeded);
        assert!(fake.state().song_id.is_none());
        let snapshot = handle.snapshot().value.expect("snapshot value");
        assert!(snapshot["queue"]["items"].as_array().unwrap().is_empty());
        assert!(fake.calls().contains(&AudioCall::Stop));

        handle.shutdown();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn platform_focus_inputs_are_serialized_by_the_runtime() {
        let data_dir = test_dir("platform-focus");
        let core = Arc::new(StereodromeCore::new(&data_dir).expect("core initializes"));
        let fake = Arc::new(FakeAudio::default());
        fake.play("song-a").expect("fake playback starts");
        let audio: Arc<dyn AudioPort> = fake.clone();
        let handle = StereodromeRuntimeHandle::start_with_core_and_audio(&data_dir, core, audio)
            .expect("runtime starts");

        let lost = handle.dispatch_command(CoreCommand::ReportPlatformPlayback {
            event: crate::PlatformPlaybackEvent::AudioFocusLost { transient: true },
        });
        assert_eq!(lost.status, CommandStatus::Succeeded);
        assert!(!fake.state().is_playing);
        let gained = handle.dispatch_command(CoreCommand::ReportPlatformPlayback {
            event: crate::PlatformPlaybackEvent::AudioFocusGained,
        });
        assert_eq!(gained.status, CommandStatus::Succeeded);
        assert!(fake.state().is_playing);

        handle.shutdown();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn stale_playback_preparation_cannot_touch_the_audio_engine() {
        let data_dir = test_dir("stale-playback");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let fake = FakeAudio::default();
        let connectivity = initial_connectivity(&core).expect("connectivity initializes");
        let mut state = CoreState::new(connectivity);
        let stale_operation = OperationId(1);
        state.playback_operation_id = Some(OperationId(2));
        let runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let task = runtime.spawn(std::future::pending::<()>());
        let mut pending = HashMap::from([(
            stale_operation,
            PendingPlayback {
                command_id: CommandId(8),
                command: CoreCommand::ResumePlayback,
                response: None,
                cancellation: CancellationToken::new(),
                abort_handle: task.abort_handle(),
                success_value: None,
            },
        )]);
        let prepared = playback::PreparedPlayback {
            target_song_id: "stale-song".to_string(),
            prepared: PreparedAudio {
                audio_path: PathBuf::from("stale-song.mp3"),
                metadata: stereodrome_audio::SongMetadata {
                    id: "stale-song".to_string(),
                    title: "Stale".to_string(),
                    artist: "Artist".to_string(),
                    album: "Album".to_string(),
                    cover_art_id: None,
                },
                duration_seconds: 180.0,
                normalization_gain: None,
                dynamics_preset: None,
                binaural_preset: None,
                equalizer_settings: None,
            },
            commit: playback::PlaybackCommit::Current {
                seek_seconds: None,
                pause_after_start: false,
            },
        };
        let (events, _) = broadcast::channel(4);
        let (mailbox, _receiver) = mpsc::sync_channel(4);
        let mut next_event_id = 1;
        let mut next_internal_command_id = 10;
        let mut result_cache = ResultCache::new();
        complete_playback(
            stale_operation,
            Ok(prepared),
            &core,
            &fake,
            &events,
            1,
            &mut state,
            &mut next_event_id,
            &mailbox,
            &mut next_internal_command_id,
            &mut pending,
            &mut result_cache,
        );

        assert!(fake.calls().is_empty());
        assert_eq!(state.playback_operation_id, Some(OperationId(2)));
        runtime.shutdown_timeout(Duration::from_millis(50));
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn superseded_playback_cancellation_is_not_reported_as_a_failure() {
        let data_dir = test_dir("silent-playback-cancellation");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let fake = FakeAudio::default();
        let connectivity = initial_connectivity(&core).expect("connectivity initializes");
        let mut state = CoreState::new(connectivity);
        let operation_id = OperationId(1);
        state.playback_operation_id = Some(operation_id);
        state.operations.insert(
            operation_id,
            OperationSnapshot {
                operation_id,
                cause_command_id: CommandId(8),
                kind: JobKind::PlaybackPrepare {
                    song_id: "song-1".to_string(),
                },
                phase: OperationPhase::Running,
            },
        );
        let runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let task = runtime.spawn(std::future::pending::<()>());
        let cancellation = CancellationToken::new();
        let cancellation_probe = cancellation.clone();
        let (response, response_receiver) = mpsc::channel();
        let mut pending = HashMap::from([(
            operation_id,
            PendingPlayback {
                command_id: CommandId(8),
                command: CoreCommand::NavigatePlayback {
                    navigation: crate::PlaybackNavigation::Next { force: true },
                },
                response: Some(response),
                cancellation,
                abort_handle: task.abort_handle(),
                success_value: None,
            },
        )]);
        let (events, mut event_receiver) = broadcast::channel(4);
        let mut next_event_id = 1;
        let mut result_cache = ResultCache::new();

        assert!(cancel_pending_playback(
            operation_id,
            &core,
            &fake,
            &events,
            1,
            &mut state,
            &mut next_event_id,
            &mut pending,
            &mut result_cache,
            CancellationReporting::Silent,
        ));

        assert!(cancellation_probe.is_cancelled());
        assert!(state.last_failure.is_none());
        assert!(state.operations.is_empty());
        assert!(state.playback_operation_id.is_none());
        let result = response_receiver
            .recv()
            .expect("caller receives cancellation");
        assert!(matches!(
            result.error,
            Some(ProtocolError {
                code: ProtocolErrorCode::Cancelled,
                ..
            })
        ));
        assert!(matches!(
            event_receiver.try_recv().expect("snapshot event"),
            CoreEvent {
                kind: CoreEventKind::SnapshotChanged { .. },
                ..
            }
        ));
        assert!(matches!(
            event_receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));

        runtime.shutdown_timeout(Duration::from_millis(50));
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn invalidated_prefetch_cancellation_is_not_reported_as_a_failure() {
        let data_dir = test_dir("silent-prefetch-cancellation");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let fake = FakeAudio::default();
        let connectivity = initial_connectivity(&core).expect("connectivity initializes");
        let mut state = CoreState::new(connectivity);
        let operation_id = OperationId(1);
        state.operations.insert(
            operation_id,
            OperationSnapshot {
                operation_id,
                cause_command_id: CommandId(9),
                kind: JobKind::QueuePrefetch,
                phase: OperationPhase::Running,
            },
        );
        let runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let task = runtime.spawn(std::future::pending::<()>());
        let cancellation = CancellationToken::new();
        let cancellation_probe = cancellation.clone();
        let mut pending = HashMap::from([(
            operation_id,
            PendingEffect {
                command_id: CommandId(9),
                command: CoreCommand::StartQueuePrefetch {
                    reserve_first: false,
                },
                response: None,
                cancellation,
                abort_handle: task.abort_handle(),
            },
        )]);
        let (events, mut event_receiver) = broadcast::channel(4);
        let mut next_event_id = 1;
        let mut result_cache = ResultCache::new();

        assert!(cancel_pending_effect(
            operation_id,
            &core,
            &fake,
            &events,
            1,
            &mut state,
            &mut next_event_id,
            &mut pending,
            &mut result_cache,
            CancellationReporting::Silent,
        ));

        assert!(cancellation_probe.is_cancelled());
        assert!(state.last_failure.is_none());
        assert!(state.operations.is_empty());
        assert!(matches!(
            event_receiver.try_recv().expect("snapshot event"),
            CoreEvent {
                kind: CoreEventKind::SnapshotChanged { .. },
                ..
            }
        ));
        assert!(matches!(
            event_receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));

        runtime.shutdown_timeout(Duration::from_millis(50));
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
