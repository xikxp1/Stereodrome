package expo.modules.stereodromecore

import org.json.JSONObject
import kotlin.math.abs

private const val POSITION_DEDUPLICATION_TOLERANCE_SECONDS = 0.25

sealed class NowPlayingProjection {
  data object Invalid : NowPlayingProjection()
  data object Stopped : NowPlayingProjection()
  data class Active(val info: NowPlayingInfo) : NowPlayingProjection()
}

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
  val outputState: String = "closed",
) {
  fun hasSameProjection(other: NowPlayingInfo): Boolean {
    val positionMatches = positionSeconds == other.positionSeconds ||
      (isPlaying &&
        other.isPlaying &&
        abs(positionSeconds - other.positionSeconds) < POSITION_DEDUPLICATION_TOLERANCE_SECONDS)
    return songId == other.songId &&
      title == other.title &&
      artist == other.artist &&
      album == other.album &&
      durationSeconds == other.durationSeconds &&
      positionMatches &&
      isPlaying == other.isPlaying &&
      artworkUri == other.artworkUri &&
      queueIndex == other.queueIndex &&
      queueCount == other.queueCount &&
      canNext == other.canNext &&
      canPlay == other.canPlay &&
      canPrevious == other.canPrevious &&
      canSeek == other.canSeek &&
      outputState == other.outputState
  }

  companion object {
    fun fromRuntimeEventJson(raw: String): NowPlayingProjection {
      val event = try {
        JSONObject(raw)
      } catch (_: Exception) {
        return NowPlayingProjection.Invalid
      }
      if (event.optInt("protocol_version", -1) != 1) {
        return NowPlayingProjection.Invalid
      }
      val kind = event.optJSONObject("kind") ?: return NowPlayingProjection.Invalid
      if (kind.optString("type") != "snapshot-changed") {
        return NowPlayingProjection.Invalid
      }
      val snapshot = kind.optJSONObject("snapshot")
        ?.optJSONObject("playback") ?: return NowPlayingProjection.Invalid
      if (snapshot.optString("state") == "stopped" || snapshot.isNull("song")) {
        return NowPlayingProjection.Stopped
      }
      val song = snapshot.optJSONObject("song") ?: return NowPlayingProjection.Invalid
      val songId = stringOrNull(song, "id") ?: return NowPlayingProjection.Invalid
      val duration = snapshot.optDouble("duration_seconds", 0.0).takeIf { it > 0.0 }
        ?: song.optDouble("duration_seconds", 0.0)
      return NowPlayingProjection.Active(
        NowPlayingInfo(
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
          outputState = snapshot.optString("output_state", "closed"),
        )
      )
    }
  }
}

fun hasSameNowPlayingProjection(
  current: NowPlayingInfo?,
  next: NowPlayingInfo?,
): Boolean = when {
  current == null || next == null -> current == next
  else -> current.hasSameProjection(next)
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
