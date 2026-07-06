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

    val status = parseOkValue(call("audioGetStatus", "null"))
    if (stringOrNull(status, "current_song_id") != null) {
      call("audioResume", "null")
    } else {
      playCurrentFromPersistedPosition()
    }
  }

  // Starting playback from a stopped core (e.g. media-notification play after
  // the app restored a paused session) should continue from the persisted
  // position instead of restarting the song from the beginning.
  private fun playCurrentFromPersistedPosition() {
    val snapshot = parseOkValue(call("getPlaybackState", "null"))
    val savedSongId = stringOrNull(snapshot, "current_song_id")
    val savedPosition = snapshot?.optDouble("position_seconds", 0.0) ?: 0.0

    val playStatus = parseOkValue(call("audioPlayCurrent", "null"))
    val playingSongId = stringOrNull(playStatus, "current_song_id")
    if (playingSongId == null || playingSongId != savedSongId || savedPosition <= 0.5) {
      return
    }

    val duration = playStatus?.optDouble("duration", 0.0) ?: 0.0
    val target =
      if (duration > 0.0) minOf(savedPosition, maxOf(0.0, duration - 1.0)) else savedPosition
    call("audioSeek", JSONObject.numberToString(target))
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

  // `JSONObject.optString` coerces a JSON null into the literal string
  // "null", which made null song ids look like real ones. Resolve to a
  // Kotlin null instead.
  private fun stringOrNull(obj: JSONObject?, key: String): String? {
    if (obj == null || obj.isNull(key)) {
      return null
    }
    return obj.optString(key).takeIf { it.isNotEmpty() }
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
