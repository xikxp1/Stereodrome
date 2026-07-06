package expo.modules.stereodromecore

import android.content.Context
import org.json.JSONObject

object StereodromeCoreBridge {
  private val jni = StereodromeCoreJni()
  private val lock = Any()
  private var handle: Long = 0
  @Volatile private var playbackSnapshotListener: ((String) -> Unit)? = null

  fun initialize(dataDir: String): Boolean = synchronized(lock) {
    if (handle != 0L) {
      jni.destroy(handle)
    }
    handle = jni.initialize(dataDir)
    handle != 0L
  }

  fun destroy() = synchronized(lock) {
    if (handle != 0L) {
      jni.destroy(handle)
      handle = 0
    }
  }

  fun setPlaybackSnapshotListener(listener: ((String) -> Unit)?) {
    playbackSnapshotListener = listener
  }

  @JvmStatic
  fun onRustPlaybackSnapshot(snapshot: String) {
    playbackSnapshotListener?.invoke(snapshot)
  }

  fun call(method: String, payload: String): String = synchronized(lock) {
    if (handle == 0L) {
      return """{"ok":false,"error":"Stereodrome Rust core is not initialized"}"""
    }
    jni.call(handle, method, payload)
  }

  fun hasCore(): Boolean = synchronized(lock) {
    handle != 0L
  }

  fun pauseFromAudioFocusLoss(context: Context) {
    if (!hasCore()) {
      return
    }
    call("audioPause", "null")
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
    call("playNext", "true")
    call("audioPlayCurrent", "null")
  }

  fun previous() {
    if (!hasCore()) {
      return
    }
    call("playPrevious", "null")
    call("audioPlayCurrent", "null")
  }

  fun seekTo(positionSeconds: Double) {
    if (!hasCore()) {
      return
    }
    call("audioSeek", JSONObject.numberToString(positionSeconds))
  }

}
