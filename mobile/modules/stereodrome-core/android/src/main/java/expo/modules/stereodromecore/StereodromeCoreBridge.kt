package expo.modules.stereodromecore

import android.content.Context
import android.util.Log
import org.json.JSONObject

object StereodromeCoreBridge {
  private const val TAG = "StereodromeCoreBridge"
  private val jni = StereodromeCoreJni()
  private val lock = Any()
  @Volatile private var handle: Long = 0
  private var applicationContext: Context? = null
  @Volatile private var playbackSnapshotListener: ((String) -> Unit)? = null

  fun initialize(context: Context, dataDir: String): Boolean = synchronized(lock) {
    applicationContext = context.applicationContext
    if (handle != 0L) {
      return@synchronized true
    }
    handle = jni.initialize(dataDir)
    handle != 0L
  }

  fun destroy() {
    val context = synchronized(lock) {
      val currentHandle = handle
      handle = 0
      applicationContext?.let(StereodromeAudioFocus::abandon)
      if (currentHandle != 0L) {
        jni.destroy(currentHandle)
      }
      applicationContext
    }
    context?.let(StereodromeMediaSessionState::clear)
  }

  fun setPlaybackSnapshotListener(listener: ((String) -> Unit)?) {
    playbackSnapshotListener = listener
  }

  @JvmStatic
  fun onRustPlaybackSnapshot(snapshot: String) {
    // destroy() clears handle before joining Rust's monitor thread. Do not take
    // lock here or teardown can wait on a callback that is waiting on teardown.
    if (handle == 0L) {
      return
    }
    // Apply the OS projection before returning through JNI; the Rust command
    // completes as soon as this callback returns.
    try {
      applicationContext?.let { context ->
        StereodromeMediaSessionState.applyPlaybackSnapshot(context, snapshot)
      }
    } catch (error: Throwable) {
      Log.e(TAG, "Failed to apply synchronous playback projection", error)
    }
    playbackSnapshotListener?.invoke(snapshot)
  }

  fun call(method: String, payload: String): String = synchronized(lock) {
    if (handle == 0L) {
      return """{"ok":false,"error":"Stereodrome Rust core is not initialized"}"""
    }
    jni.call(handle, method, payload)
  }

  fun callWithAudioFocus(
    context: Context,
    method: String,
    payload: String,
  ): String = synchronized(lock) {
    val lease = StereodromeAudioFocus.request(context.applicationContext)
      ?: return@synchronized errorEnvelope("Android audio focus request was denied")
    val result = if (handle == 0L) {
      errorEnvelope("Stereodrome Rust core is not initialized")
    } else {
      jni.call(handle, method, payload)
    }
    if (!isSuccessfulResponse(result)) {
      StereodromeAudioFocus.rollback(context.applicationContext, lease)
    }
    result
  }

  fun hasCore(): Boolean = synchronized(lock) {
    handle != 0L
  }

  fun pauseFromAudioFocusLoss() {
    if (!hasCore()) {
      return
    }
    if (!isPlaying()) {
      return
    }
    call("audioPause", "null")
  }

  fun pauseFromTransientAudioFocusLoss(): Boolean {
    if (!hasCore() || !isPlaying()) {
      return false
    }
    call("audioPause", "null")
    return true
  }

  fun resumeFromAudioFocusGain() {
    if (!hasCore()) {
      return
    }
    val result = call("audioResume", "null")
    if (!isSuccessfulResponse(result)) {
      applicationContext?.let(StereodromeAudioFocus::abandon)
    }
  }

  fun play() {
    if (!hasCore()) {
      return
    }

    call("audioResume", "null")
  }

  fun pause() {
    if (!hasCore()) {
      return
    }
    call("audioPause", "null")
  }

  fun stop() {
    if (!hasCore()) {
      return
    }
    call("audioStop", "null")
  }

  fun next() {
    if (!hasCore()) {
      return
    }
    call("audioPlayNext", "true")
  }

  fun previous() {
    if (!hasCore()) {
      return
    }
    call("audioPlayPrevious", "null")
  }

  fun seekTo(positionSeconds: Double) {
    if (!hasCore()) {
      return
    }
    call("audioSeek", JSONObject.numberToString(positionSeconds))
  }

  private fun isPlaying(): Boolean {
    return try {
      val envelope = JSONObject(call("getPlaybackSnapshot", "null"))
      if (envelope.optBoolean("ok", false)) {
        envelope.optJSONObject("value")?.optBoolean("is_playing", false) == true
      } else {
        false
      }
    } catch (error: Throwable) {
      false
    }
  }

  fun isSuccessfulResponse(raw: String): Boolean = try {
    JSONObject(raw).optBoolean("ok", false)
  } catch (error: Throwable) {
    false
  }

  private fun errorEnvelope(message: String): String =
    JSONObject(mapOf("ok" to false, "error" to message)).toString()
}
