package expo.modules.stereodromecore

import org.json.JSONObject

object StereodromeCoreBridge {
  private val jni = StereodromeCoreJni()
  private val lock = Any()
  private var handle: Long = 0
  private var invalidationListener: (() -> Unit)? = null

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

  fun setInvalidationListener(listener: (() -> Unit)?) {
    invalidationListener = listener
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

  fun pauseFromAudioFocusLoss() {
    if (!hasCore()) {
      return
    }
    call("audioPause", "null")
    invalidationListener?.invoke()
  }

  fun play() {
    if (!hasCore()) {
      return
    }

    val status = parseOkValue(call("audioGetStatus", "null"))
    val hasCurrentSong = status?.optString("current_song_id")?.isNotEmpty() == true
    if (hasCurrentSong) {
      call("audioResume", "null")
    } else {
      call("audioPlayCurrent", "null")
    }
    invalidationListener?.invoke()
  }

  fun pause() {
    if (!hasCore()) {
      return
    }
    call("audioPause", "null")
    invalidationListener?.invoke()
  }

  fun toggle() {
    if (!hasCore()) {
      return
    }

    val status = parseOkValue(call("audioGetStatus", "null"))
    if (status?.optBoolean("is_playing") == true) {
      call("audioPause", "null")
    } else {
      val hasCurrentSong = status?.optString("current_song_id")?.isNotEmpty() == true
      if (hasCurrentSong) {
        call("audioResume", "null")
      } else {
        call("audioPlayCurrent", "null")
      }
    }
    invalidationListener?.invoke()
  }

  fun stop() {
    if (!hasCore()) {
      return
    }
    call("audioStop", "null")
    invalidationListener?.invoke()
  }

  fun next() {
    if (!hasCore()) {
      return
    }
    call("playNext", "true")
    call("audioPlayCurrent", "null")
    invalidationListener?.invoke()
  }

  fun previous() {
    if (!hasCore()) {
      return
    }
    call("playPrevious", "null")
    call("audioPlayCurrent", "null")
    invalidationListener?.invoke()
  }

  fun seekTo(positionSeconds: Double) {
    if (!hasCore()) {
      return
    }
    call("audioSeek", JSONObject.numberToString(positionSeconds))
    invalidationListener?.invoke()
  }

  private fun parseOkValue(raw: String): JSONObject? {
    return try {
      val envelope = JSONObject(raw)
      if (!envelope.optBoolean("ok")) {
        null
      } else {
        envelope.optJSONObject("value")
      }
    } catch (_: Exception) {
      null
    }
  }
}
