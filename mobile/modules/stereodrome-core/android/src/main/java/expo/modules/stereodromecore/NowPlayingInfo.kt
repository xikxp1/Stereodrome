package expo.modules.stereodromecore

data class NowPlayingInfo(
  val songId: String,
  val title: String,
  val artist: String?,
  val album: String?,
  val durationSeconds: Double,
  val positionSeconds: Double,
  val isPlaying: Boolean,
  val artworkUri: String?,
  val queueIndex: Int?,
  val queueCount: Int,
  val canNext: Boolean,
  val canPlay: Boolean,
  val canPrevious: Boolean,
  val canSeek: Boolean,
) {
  companion object {
    fun fromPayload(payload: Map<String, Any?>): NowPlayingInfo {
      return NowPlayingInfo(
        songId = payload.stringValue("song_id") ?: "",
        title = payload.stringValue("title") ?: "Unknown Title",
        artist = payload.stringValue("artist"),
        album = payload.stringValue("album"),
        durationSeconds = payload.doubleValue("duration_seconds"),
        positionSeconds = payload.doubleValue("position_seconds"),
        isPlaying = payload.booleanValue("is_playing"),
        artworkUri = payload.stringValue("artwork_uri"),
        queueIndex = payload.intValue("queue_index"),
        queueCount = payload.intValue("queue_count") ?: 0,
        canNext = payload.booleanValue("can_next"),
        canPlay = payload.booleanValue("can_play"),
        canPrevious = payload.booleanValue("can_previous"),
        canSeek = payload.booleanValue("can_seek"),
      )
    }
  }
}

data class NowPlayingProgress(
  val songId: String?,
  val durationSeconds: Double,
  val positionSeconds: Double,
  val isPlaying: Boolean,
) {
  companion object {
    fun fromPayload(payload: Map<String, Any?>): NowPlayingProgress {
      return NowPlayingProgress(
        songId = payload.stringValue("song_id"),
        durationSeconds = payload.doubleValue("duration_seconds"),
        positionSeconds = payload.doubleValue("position_seconds"),
        isPlaying = payload.booleanValue("is_playing"),
      )
    }
  }
}

private fun Map<String, Any?>.stringValue(key: String): String? =
  this[key]?.toString()?.takeIf { it.isNotBlank() }

private fun Map<String, Any?>.doubleValue(key: String): Double =
  when (val value = this[key]) {
    is Number -> value.toDouble()
    is String -> value.toDoubleOrNull() ?: 0.0
    else -> 0.0
  }

private fun Map<String, Any?>.intValue(key: String): Int? =
  when (val value = this[key]) {
    is Number -> value.toInt()
    is String -> value.toIntOrNull()
    else -> null
  }

private fun Map<String, Any?>.booleanValue(key: String): Boolean =
  when (val value = this[key]) {
    is Boolean -> value
    is String -> value == "true"
    else -> false
  }
