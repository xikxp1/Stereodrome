//! Thin C ABI adapter for the Stereodrome runtime.

use std::ffi::{CStr, CString, c_char, c_void};
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Once};
use std::thread;

use log::{Level, LevelFilter, Metadata, Record};
use stereodrome_core::{
    CORE_PROTOCOL_VERSION, CommandId, CoreCommandRequest, CoreCommandResult, CoreEvent,
    ProtocolError, ProtocolErrorCode, StereodromeRuntimeHandle,
};

static MOBILE_LOGGER: MobileLogger = MobileLogger;
static INIT_LOGGER: Once = Once::new();
static INIT_PANIC_HOOK: Once = Once::new();
static LOG_CALLBACK: Mutex<Option<MobileLogCallback>> = Mutex::new(None);

type MobileLogCallback = extern "C" fn(*const c_char);
type MobileEventCallback = extern "C" fn(*const c_char, *mut c_void);

struct MobileLogger;

impl log::Log for MobileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = format!(
            "[stereodrome-rust][{}][{}] {}",
            record.level(),
            record.target(),
            record.args()
        );
        if let Some(callback) = LOG_CALLBACK.lock().ok().and_then(|guard| *guard)
            && let Ok(message) = CString::new(message.as_str())
        {
            callback(message.as_ptr());
            return;
        }
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(message.as_bytes());
        let _ = stderr.write_all(b"\n");
    }

    fn flush(&self) {}
}

fn init_mobile_logging() {
    INIT_LOGGER.call_once(|| {
        if log::set_logger(&MOBILE_LOGGER).is_ok() {
            // Debug records cross the FFI boundary and reach platform logs;
            // release builds cap at Info to keep that path quiet.
            log::set_max_level(if cfg!(debug_assertions) {
                LevelFilter::Debug
            } else {
                LevelFilter::Info
            });
        }
    });
    INIT_PANIC_HOOK.call_once(|| {
        panic::set_hook(Box::new(|panic_info| {
            let location = panic_info.location().map_or_else(
                || "unknown location".to_string(),
                |location| {
                    format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                },
            );
            log::error!(
                target: "stereodrome_ffi",
                "Rust panic at {location}: {}",
                panic_payload_message(panic_info.payload())
            );
        }));
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_runtime_set_log_callback(callback: Option<MobileLogCallback>) {
    init_mobile_logging();
    if let Ok(mut current) = LOG_CALLBACK.lock() {
        *current = callback;
    }
}

#[derive(Clone, Copy)]
struct InstanceEventCallback {
    callback: MobileEventCallback,
    context: usize,
}

#[derive(Default)]
struct EventEmitterState {
    callback: Option<InstanceEventCallback>,
    applied_revision: Option<u64>,
}

#[derive(Clone, Default)]
struct MobileEventEmitter {
    state: Arc<(Mutex<EventEmitterState>, Condvar)>,
}

impl MobileEventEmitter {
    fn set_callback(&self, callback: Option<MobileEventCallback>, context: *mut c_void) {
        let (state, applied) = self.state.as_ref();
        if let Ok(mut current) = state.lock() {
            current.callback = callback.map(|callback| InstanceEventCallback {
                callback,
                context: context.addr(),
            });
            if current.callback.is_some() {
                current.applied_revision = None;
            }
            applied.notify_all();
        }
    }

    fn emit(&self, event: &CoreEvent) {
        let message = match serde_json::to_string(event)
            .map_err(|error| error.to_string())
            .and_then(|json| CString::new(json).map_err(|error| error.to_string()))
        {
            Ok(message) => message,
            Err(error) => {
                log::warn!(target: "stereodrome_ffi", "Failed to serialize runtime event: {error}");
                return;
            }
        };
        let (state, applied) = self.state.as_ref();
        // Holding the guard through invocation makes clearing the callback a
        // synchronous lifetime barrier for the opaque context pointer.
        if let Ok(mut current) = state.lock()
            && let Some(callback) = current.callback
        {
            (callback.callback)(
                message.as_ptr(),
                ptr::with_exposed_provenance_mut(callback.context),
            );
            current.applied_revision = Some(event.revision);
            applied.notify_all();
        }
    }

    /// Waits until the native callback has synchronously applied an event at
    /// least as new as `revision`. Clearing the callback releases waiters
    /// during teardown.
    fn wait_until_applied(&self, revision: u64) {
        let (state, applied) = self.state.as_ref();
        let Ok(mut current) = state.lock() else {
            return;
        };
        while current.callback.is_some()
            && current
                .applied_revision
                .is_none_or(|applied_revision| applied_revision < revision)
        {
            current = match applied.wait(current) {
                Ok(current) => current,
                Err(_) => return,
            };
        }
    }
}

pub struct MobileRuntime {
    runtime: StereodromeRuntimeHandle,
    event_emitter: MobileEventEmitter,
    event_thread_running: Arc<AtomicBool>,
    event_thread: Option<thread::JoinHandle<()>>,
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_runtime_new(data_dir: *const c_char) -> *mut MobileRuntime {
    init_mobile_logging();
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let Some(data_dir) = read_c_string(data_dir) else {
            return ptr::null_mut();
        };
        let data_dir = PathBuf::from(data_dir);
        let Ok(runtime) = StereodromeRuntimeHandle::start(&data_dir) else {
            return ptr::null_mut();
        };
        let event_emitter = MobileEventEmitter::default();
        let event_thread_running = Arc::new(AtomicBool::new(true));
        let event_thread = start_runtime_event_bridge(
            runtime.subscribe(),
            event_emitter.clone(),
            Arc::clone(&event_thread_running),
        );
        Box::into_raw(Box::new(MobileRuntime {
            runtime,
            event_emitter,
            event_thread_running,
            event_thread: Some(event_thread),
        }))
    })) {
        Ok(runtime) => runtime,
        Err(payload) => {
            log::error!(
                target: "stereodrome_ffi",
                "Rust panic while initializing runtime: {}",
                panic_payload_message(payload.as_ref())
            );
            ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_runtime_set_event_callback(
    runtime: *mut MobileRuntime,
    callback: Option<MobileEventCallback>,
    context: *mut c_void,
) {
    if let Some(runtime) = runtime_ref(runtime) {
        runtime.event_emitter.set_callback(callback, context);
    }
}

/// # Safety
///
/// `runtime` must be a pointer returned by [`stereodrome_runtime_new`] and must
/// not be used after this function returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stereodrome_runtime_destroy(runtime: *mut MobileRuntime) {
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        if runtime.is_null() {
            return;
        }
        unsafe {
            let MobileRuntime {
                runtime,
                event_emitter,
                event_thread_running,
                event_thread,
            } = *Box::from_raw(runtime);
            event_emitter.set_callback(None, ptr::null_mut());
            runtime.shutdown();
            event_thread_running.store(false, Ordering::SeqCst);
            // Dropping the handle drops the last event sender, closing the
            // channel so the bridge thread's blocking receive returns.
            drop(runtime);
            if let Some(event_thread) = event_thread
                && event_thread.join().is_err()
            {
                log::warn!(target: "stereodrome_ffi", "Runtime event bridge panicked during shutdown");
            }
        }
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_runtime_dispatch(
    runtime: *mut MobileRuntime,
    command_json: *const c_char,
) -> *mut c_char {
    catch_protocol_response(|| {
        let Some(runtime) = runtime_ref(runtime) else {
            return protocol_input_error("runtime is not initialized");
        };
        let Some(command_json) = read_c_string(command_json) else {
            return protocol_input_error("command JSON is required");
        };
        let request = match serde_json::from_str::<CoreCommandRequest>(&command_json) {
            Ok(request) => request,
            Err(error) => return protocol_input_error(&format!("invalid command JSON: {error}")),
        };
        let waits_for_event = request.command.emits_event_before_completion();
        let result = runtime.runtime.dispatch(request);
        if waits_for_event && result.status == stereodrome_core::CommandStatus::Succeeded {
            runtime
                .event_emitter
                .wait_until_applied(result.accepted_revision);
        }
        result
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn stereodrome_runtime_snapshot(runtime: *mut MobileRuntime) -> *mut c_char {
    catch_protocol_response(|| {
        runtime_ref(runtime).map_or_else(
            || protocol_input_error("runtime is not initialized"),
            |runtime| runtime.runtime.snapshot(),
        )
    })
}

/// # Safety
///
/// `value` must have been returned by this library and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stereodrome_string_free(value: *mut c_char) {
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        if !value.is_null() {
            unsafe {
                let _ = CString::from_raw(value);
            }
        }
    }));
}

fn start_runtime_event_bridge(
    mut events: tokio::sync::broadcast::Receiver<CoreEvent>,
    event_emitter: MobileEventEmitter,
    running: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    // Blocks until an event arrives; the channel closes once the runtime
    // handle is dropped in stereodrome_runtime_destroy, which unblocks the
    // final join without any polling.
    thread::spawn(move || {
        while running.load(Ordering::SeqCst) {
            match events.blocking_recv() {
                Ok(event) => event_emitter.emit(&event),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    log::warn!(target: "stereodrome_ffi", "Runtime event bridge skipped {skipped} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

fn catch_protocol_response(operation: impl FnOnce() -> CoreCommandResult) -> *mut c_char {
    let result = match panic::catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(payload) => {
            let message = panic_payload_message(payload.as_ref());
            log::error!(target: "stereodrome_ffi", "Rust panic while handling runtime FFI call: {message}");
            CoreCommandResult::failed(
                CommandId(0),
                0,
                None,
                ProtocolError::new(
                    ProtocolErrorCode::Internal,
                    format!("Rust panic: {message}"),
                    false,
                ),
            )
        }
    };
    into_c_string(serialize_protocol_result(&result))
}

fn protocol_input_error(message: &str) -> CoreCommandResult {
    CoreCommandResult::failed(
        CommandId(0),
        0,
        None,
        ProtocolError::new(ProtocolErrorCode::InvalidInput, message, false),
    )
}

fn serialize_protocol_result(result: &CoreCommandResult) -> String {
    serde_json::to_string(result).unwrap_or_else(|error| {
        serde_json::json!({
            "protocol_version": CORE_PROTOCOL_VERSION,
            "command_id": 0,
            "accepted_revision": 0,
            "operation_id": null,
            "status": "failed",
            "error": {
                "code": "internal",
                "message": format!("failed to serialize command result: {error}"),
                "retryable": false
            }
        })
        .to_string()
    })
}

fn runtime_ref<'a>(runtime: *mut MobileRuntime) -> Option<&'a MobileRuntime> {
    if runtime.is_null() {
        None
    } else {
        unsafe { runtime.as_ref() }
    }
}

fn read_c_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(value).to_str().ok().map(ToOwned::to_owned) }
}

fn into_c_string(value: String) -> *mut c_char {
    CString::new(value).map_or(ptr::null_mut(), CString::into_raw)
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "non-string panic payload".to_string()
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "FFI tests intentionally use fail-fast setup and assertions"
)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    use std::sync::mpsc;
    use std::time::Duration;
    use stereodrome_core::{CommandStatus, CoreCommand, CoreEventKind};

    static CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);

    extern "C" fn event_callback(message: *const c_char, context: *mut c_void) {
        assert!(!message.is_null());
        assert_eq!(context.addr(), 41);
        let event: CoreEvent = serde_json::from_str(
            unsafe { CStr::from_ptr(message) }
                .to_str()
                .expect("callback event is UTF-8"),
        )
        .expect("callback event matches the runtime protocol");
        assert!(matches!(event.kind, CoreEventKind::RuntimeShuttingDown));
        CALLBACK_COUNT.fetch_add(1, AtomicOrdering::SeqCst);
    }

    struct BlockingCallback {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    extern "C" fn blocking_event_callback(message: *const c_char, context: *mut c_void) {
        assert!(!message.is_null());
        assert!(!context.is_null());
        let callback = unsafe { &*context.cast::<BlockingCallback>() };
        callback.entered.send(()).expect("test observes callback");
        callback
            .release
            .lock()
            .expect("release receiver locks")
            .recv()
            .expect("test releases callback");
    }

    struct TestRuntime {
        pointer: *mut MobileRuntime,
        data_dir: PathBuf,
    }

    impl TestRuntime {
        fn new(name: &str) -> Self {
            static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);
            let test_id = NEXT_TEST_ID.fetch_add(1, AtomicOrdering::Relaxed);
            let data_dir = std::env::temp_dir().join(format!(
                "stereodrome-ffi-{name}-{}-{test_id}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&data_dir);
            let data_dir_string = CString::new(data_dir.to_string_lossy().as_bytes())
                .expect("test data directory has no null bytes");
            let pointer = stereodrome_runtime_new(data_dir_string.as_ptr());
            assert!(!pointer.is_null(), "runtime initializes");
            Self { pointer, data_dir }
        }

        fn dispatch(&self, request: &CoreCommandRequest) -> CoreCommandResult {
            let request = CString::new(serde_json::to_string(request).expect("request serializes"))
                .expect("request has no null bytes");
            let response = stereodrome_runtime_dispatch(self.pointer, request.as_ptr());
            assert!(!response.is_null(), "runtime FFI returns a response");
            let response_json = unsafe { CStr::from_ptr(response) }
                .to_str()
                .expect("runtime response is UTF-8")
                .to_string();
            unsafe { stereodrome_string_free(response) };
            serde_json::from_str(&response_json).expect("runtime response is JSON")
        }
    }

    impl Drop for TestRuntime {
        fn drop(&mut self) {
            unsafe { stereodrome_runtime_destroy(self.pointer) };
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }

    #[test]
    fn typed_ffi_dispatches_and_rejects_protocol_mismatches() {
        let runtime = TestRuntime::new("typed-dispatch");
        let snapshot = runtime.dispatch(&CoreCommandRequest {
            protocol_version: CORE_PROTOCOL_VERSION,
            command_id: CommandId(1),
            command: CoreCommand::GetSnapshot,
        });
        assert_eq!(snapshot.status, CommandStatus::Succeeded);

        let mismatch = runtime.dispatch(&CoreCommandRequest {
            protocol_version: CORE_PROTOCOL_VERSION + 1,
            command_id: CommandId(2),
            command: CoreCommand::GetSnapshot,
        });
        assert_eq!(mismatch.status, CommandStatus::Failed);
        assert_eq!(
            mismatch.error.expect("structured error").code,
            ProtocolErrorCode::UnsupportedProtocolVersion
        );
    }

    #[test]
    fn event_callback_receives_the_unwrapped_runtime_event() {
        CALLBACK_COUNT.store(0, AtomicOrdering::SeqCst);
        let emitter = MobileEventEmitter::default();
        emitter.set_callback(
            Some(event_callback),
            ptr::with_exposed_provenance_mut(41_usize),
        );
        emitter.emit(&CoreEvent {
            protocol_version: CORE_PROTOCOL_VERSION,
            stream_id: 3,
            event_id: 4,
            revision: 5,
            cause_command_id: CommandId(6),
            operation_id: None,
            kind: CoreEventKind::RuntimeShuttingDown,
        });
        assert_eq!(CALLBACK_COUNT.load(AtomicOrdering::SeqCst), 1);
        emitter.set_callback(None, ptr::null_mut());
    }

    #[test]
    fn state_changing_dispatch_waits_for_native_projection_application() {
        let runtime = TestRuntime::new("native-projection-ack");
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let mut callback = Box::new(BlockingCallback {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        });
        unsafe {
            (*runtime.pointer).event_emitter.set_callback(
                Some(blocking_event_callback),
                ptr::from_mut(callback.as_mut()).cast(),
            );
        }

        let pointer_address = runtime.pointer.addr();
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let dispatch_thread = thread::spawn(move || {
            let request = CoreCommandRequest {
                protocol_version: CORE_PROTOCOL_VERSION,
                command_id: CommandId(1),
                command: CoreCommand::Initialize,
            };
            let request =
                CString::new(serde_json::to_string(&request).expect("request serializes"))
                    .expect("request has no null bytes");
            let pointer = ptr::with_exposed_provenance_mut::<MobileRuntime>(pointer_address);
            let response = stereodrome_runtime_dispatch(pointer, request.as_ptr());
            assert!(!response.is_null(), "runtime FFI returns a response");
            let response_json = unsafe { CStr::from_ptr(response) }
                .to_str()
                .expect("runtime response is UTF-8")
                .to_string();
            unsafe { stereodrome_string_free(response) };
            result_tx
                .send(
                    serde_json::from_str::<CoreCommandResult>(&response_json)
                        .expect("runtime response is JSON"),
                )
                .expect("test receives dispatch result");
        });

        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("native callback starts");
        let early_result = result_rx.recv_timeout(Duration::from_millis(100)).ok();
        let returned_early = early_result.is_some();
        release_tx.send(()).expect("callback is released");
        let result = early_result.unwrap_or_else(|| {
            result_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("dispatch completes after callback")
        });
        dispatch_thread.join().expect("dispatch thread exits");

        assert!(!returned_early, "dispatch must wait for native state");
        assert_eq!(result.status, CommandStatus::Succeeded);
        unsafe {
            (*runtime.pointer)
                .event_emitter
                .set_callback(None, ptr::null_mut());
        }
    }
}
