package expo.modules.stereodromecore

import org.json.JSONObject

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
    fun fromSnapshotJson(raw: String): NowPlayingInfo? {
      val snapshot = try {
        JSONObject(raw)
      } catch (_: Exception) {
        return null
      }
      if (snapshot.optString("state") == "stopped" || snapshot.isNull("song")) {
        return null
      }
      val song = snapshot.optJSONObject("song") ?: return null
      val songId = stringOrNull(song, "id") ?: return null
      val duration = snapshot.optDouble("duration_seconds", 0.0).takeIf { it > 0.0 }
        ?: song.optDouble("duration_seconds", 0.0)
      return NowPlayingInfo(
        songId = songId,
        title = stringOrNull(song, "title") ?: "Unknown Title",
        artist = stringOrNull(song, "artist"),
        album = stringOrNull(song, "album"),
        durationSeconds = duration,
        positionSeconds = snapshot.optDouble("position_seconds", 0.0),
        isPlaying = snapshot.optBoolean("is_playing", false),
        artworkUri = stringOrNull(song, "artwork_uri"),
        queueIndex = intOrNull(snapshot, "queue_index"),
        queueCount = snapshot.optInt("queue_length", 0),
        canNext = snapshot.optBoolean("can_next", false),
        canPlay = snapshot.optBoolean("can_play", false),
        canPrevious = snapshot.optBoolean("can_previous", false),
        canSeek = snapshot.optBoolean("can_seek", false),
      )
    }
  }
}

private fun stringOrNull(obj: JSONObject, key: String): String? {
  if (obj.isNull(key)) {
    return null
  }
  return obj.optString(key).takeIf { it.isNotEmpty() }
}

private fun intOrNull(obj: JSONObject, key: String): Int? {
  if (obj.isNull(key)) {
    return null
  }
  return obj.optInt(key)
}
