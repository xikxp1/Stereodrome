package expo.modules.stereodromecore

import android.content.Context
import android.util.Log
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicLong

object StereodromeCoreBridge {
  private const val TAG = "StereodromeCoreBridge"
  private val jni = StereodromeCoreJni()
  private val lock = Any()
  private val nextCallbackToken = AtomicLong(1)
  private val nextNativeCommandId = AtomicLong(Long.MAX_VALUE)
  @Volatile private var handle: Long = 0
  private var applicationContext: Context? = null
  @Volatile private var coreEventListener: ((String) -> Unit)? = null
  @Volatile private var activeCallbackToken: Long = 0

  fun initialize(context: Context, dataDir: String): Boolean = synchronized(lock) {
    applicationContext = context.applicationContext
    if (handle != 0L) {
      return@synchronized true
    }
    val callbackToken = nextCallbackToken.getAndIncrement()
    activeCallbackToken = callbackToken
    handle = jni.initialize(dataDir, callbackToken)
    if (handle == 0L) {
      activeCallbackToken = 0
    }
    handle != 0L
  }

  fun destroy() {
    val context = synchronized(lock) {
      val currentHandle = handle
      handle = 0
      activeCallbackToken = 0
      applicationContext?.let(StereodromeAudioFocus::abandon)
      if (currentHandle != 0L) {
        jni.destroy(currentHandle)
      }
      applicationContext
    }
    context?.let(StereodromeMediaSessionState::clear)
  }

  fun setCoreEventListener(listener: ((String) -> Unit)?) {
    coreEventListener = listener
  }

  @JvmStatic
  fun onRustCoreEvent(callbackToken: Long, event: String) {
    if (callbackToken != activeCallbackToken) {
      return
    }
    try {
      val envelope = JSONObject(event)
      if (envelope.optString("type") == "platform-projection") {
        applicationContext?.let { context ->
          StereodromeMediaSessionState.applyPlatformEvent(context, event)
        }
      }
    } catch (error: Throwable) {
      Log.e(TAG, "Failed to apply platform projection", error)
    }
    coreEventListener?.invoke(event)
  }

  fun call(method: String, payload: String): String = synchronized(lock) {
    if (handle == 0L) {
      return """{"ok":false,"error":"Stereodrome Rust core is not initialized"}"""
    }
    jni.call(handle, method, payload)
  }

  fun dispatch(commandJson: String): String = synchronized(lock) {
    if (handle == 0L) {
      return """{"protocol_version":1,"command_id":0,"accepted_revision":0,"operation_id":null,"status":"failed","error":{"code":"runtime_unavailable","message":"Stereodrome Rust core is not initialized","retryable":true}}"""
    }
    jni.dispatch(handle, commandJson)
  }

  fun dispatchWithAudioFocus(context: Context, command: JSONObject): String = synchronized(lock) {
    val lease = StereodromeAudioFocus.request(context.applicationContext)
      ?: return@synchronized runtimeError("Android audio focus request was denied")
    val result = if (handle == 0L) {
      runtimeError("Stereodrome Rust core is not initialized")
    } else {
      dispatchCommandLocked(command)
    }
    if (!isSuccessfulRuntimeResponse(result)) {
      StereodromeAudioFocus.rollback(context.applicationContext, lease)
    }
    result
  }

  fun hasCore(): Boolean = synchronized(lock) {
    handle != 0L
  }

  fun reportAudioFocusLost(transient: Boolean) {
    if (!hasCore()) {
      return
    }
    dispatchCommand(
      JSONObject()
        .put("type", "report-platform-playback")
        .put(
          "event",
          JSONObject().put("type", "audio-focus-lost").put("transient", transient),
        ),
    )
  }

  fun reportAudioFocusGained() {
    if (!hasCore()) {
      return
    }
    val result = dispatchCommand(
      JSONObject()
        .put("type", "report-platform-playback")
        .put("event", JSONObject().put("type", "audio-focus-gained")),
    )
    if (!isSuccessfulRuntimeResponse(result)) {
      applicationContext?.let(StereodromeAudioFocus::abandon)
    }
  }

  fun dispatchCommand(command: JSONObject): String = synchronized(lock) {
    if (handle == 0L) {
      return@synchronized runtimeError("Stereodrome Rust core is not initialized")
    }
    dispatchCommandLocked(command)
  }

  fun isSuccessfulRuntimeResponse(raw: String): Boolean = try {
    JSONObject(raw).optString("status") == "succeeded"
  } catch (error: Throwable) {
    false
  }

  private fun dispatchCommandLocked(command: JSONObject): String =
    jni.dispatch(
      handle,
      JSONObject()
        .put("protocol_version", 1)
        .put("command_id", nextNativeCommandId.getAndDecrement())
        .put("command", command)
        .toString(),
    )

  private fun runtimeError(message: String): String = JSONObject()
    .put("protocol_version", 1)
    .put("command_id", 0)
    .put("accepted_revision", 0)
    .put("operation_id", JSONObject.NULL)
    .put("status", "failed")
    .put(
      "error",
      JSONObject()
        .put("code", "runtime_unavailable")
        .put("message", message)
        .put("retryable", true),
    )
    .toString()
}
