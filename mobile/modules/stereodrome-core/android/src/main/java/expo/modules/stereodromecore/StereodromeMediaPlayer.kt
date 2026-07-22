package expo.modules.stereodromecore

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.SimpleBasePlayer.MediaItemData
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import java.util.concurrent.FutureTask
import kotlin.math.max
import org.json.JSONObject

class StereodromeMediaPlayer(
  context: Context,
  private val playerLooper: Looper,
) : SimpleBasePlayer(playerLooper) {
  private val appContext = context.applicationContext
  private val handler = Handler(playerLooper)
  @Volatile private var info: NowPlayingInfo? = null

  override fun getState(): State {
    val currentInfo = info
    val builder = State.Builder()
      .setAvailableCommands(availableCommands(currentInfo))
      .setPlaybackState(if (currentInfo == null) Player.STATE_IDLE else Player.STATE_READY)
      .setPlayWhenReady(
        currentInfo?.isPlaying == true,
        Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST,
      )
      .setContentPositionMs(secondsToMillis(currentInfo?.positionSeconds ?: 0.0))

    if (currentInfo != null) {
      builder
        .setPlaylist(ImmutableList.of(mediaItemData(currentInfo)))
        .setCurrentMediaItemIndex(0)
    }

    return builder.build()
  }

  fun setNowPlayingInfo(nextInfo: NowPlayingInfo?) {
    setNowPlayingInfoOnPlayerLooper(nextInfo)
  }

  fun clearNowPlayingInfo() {
    setNowPlayingInfoOnPlayerLooper(null)
  }

  fun hasProjection(nextInfo: NowPlayingInfo?): Boolean =
    hasSameNowPlayingProjection(info, nextInfo)

  override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<Any> {
    if (playWhenReady) {
      if (info?.canPlay != true) {
        invalidateOnPlayerLooper()
        return Futures.immediateFuture(Any())
      }
      StereodromeCoreCommandQueue.enqueue("resume-playback") {
        StereodromeCoreBridge.dispatchWithAudioFocus(
          appContext,
          JSONObject().put("type", "resume-playback"),
        )
      }
    } else {
      StereodromeCoreCommandQueue.enqueue("pause-playback") {
        StereodromeCoreBridge.dispatchCommand(JSONObject().put("type", "pause-playback"))
      }
    }
    return Futures.immediateFuture(Any())
  }

  override fun handleSeek(
    mediaItemIndex: Int,
    positionMs: Long,
    seekCommand: Int,
  ): ListenableFuture<Any> {
    when (seekCommand) {
      Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> {
        if (info?.canPlay != true) {
          return Futures.immediateFuture(Any())
        }
        StereodromeCoreCommandQueue.enqueue("navigate-next") {
          StereodromeCoreBridge.dispatchWithAudioFocus(
            appContext,
            JSONObject()
              .put("type", "navigate-playback")
              .put("navigation", JSONObject().put("type", "next").put("force", true)),
          )
        }
      }
      Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> {
        if (info?.canPlay != true) {
          return Futures.immediateFuture(Any())
        }
        StereodromeCoreCommandQueue.enqueue("navigate-previous") {
          StereodromeCoreBridge.dispatchWithAudioFocus(
            appContext,
            JSONObject()
              .put("type", "navigate-playback")
              .put("navigation", JSONObject().put("type", "previous")),
          )
        }
      }
      Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM -> {
        val nextPositionSeconds = max(0.0, positionMs / 1000.0)
        StereodromeCoreCommandQueue.enqueue("seek-to") {
          StereodromeCoreBridge.dispatchCommand(
            JSONObject().put("type", "seek-to").put("seconds", nextPositionSeconds),
          )
        }
      }
    }
    return Futures.immediateFuture(Any())
  }

  override fun handleStop(): ListenableFuture<Any> {
    StereodromeCoreCommandQueue.enqueue("stop-playback") {
      StereodromeCoreBridge.dispatchCommand(JSONObject().put("type", "stop-playback"))
    }
    return Futures.immediateFuture(Any())
  }

  private fun availableCommands(currentInfo: NowPlayingInfo?): Player.Commands {
    val builder = Player.Commands.Builder()
      .add(Player.COMMAND_GET_CURRENT_MEDIA_ITEM)
      .add(Player.COMMAND_GET_METADATA)
      .add(Player.COMMAND_GET_TIMELINE)
      .add(Player.COMMAND_STOP)

    if (currentInfo?.canPlay == true || currentInfo?.isPlaying == true) {
      builder.add(Player.COMMAND_PLAY_PAUSE)
    }

    if (currentInfo?.canSeek == true) {
      builder.add(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM)
    }
    if (currentInfo != null && currentInfo.canPlay && currentInfo.canNext) {
      builder.add(Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM)
    }
    if (currentInfo != null && currentInfo.canPlay && currentInfo.canPrevious) {
      builder.add(Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM)
    }

    return builder.build()
  }

  private fun mediaItemData(currentInfo: NowPlayingInfo): MediaItemData {
    val metadataBuilder = MediaMetadata.Builder()
      .setTitle(currentInfo.title)
      .setArtist(currentInfo.artist)
      .setAlbumTitle(currentInfo.album)

    currentInfo.artworkUri?.let { metadataBuilder.setArtworkUri(Uri.parse(it)) }

    val durationUs =
      if (currentInfo.durationSeconds > 0.0) secondsToMillis(currentInfo.durationSeconds) * 1000
      else C.TIME_UNSET

    return MediaItemData.Builder(currentInfo.songId)
      .setMediaMetadata(metadataBuilder.build())
      .setDurationUs(durationUs)
      .setIsSeekable(currentInfo.canSeek)
      .build()
  }

  private fun secondsToMillis(seconds: Double): Long = (seconds * 1000).toLong()

  private fun setNowPlayingInfoOnPlayerLooper(nextInfo: NowPlayingInfo?) {
    if (Looper.myLooper() == playerLooper) {
      info = nextInfo
      invalidateState()
      return
    }
    // Snapshot delivery must not return until Media3 observes the new state.
    val task = FutureTask<Unit> {
      info = nextInfo
      invalidateState()
    }
    check(handler.post(task)) {
      "Failed to schedule media-session state on the player looper"
    }
    task.get()
  }

  private fun invalidateOnPlayerLooper() {
    if (Looper.myLooper() == playerLooper) {
      invalidateState()
    } else {
      handler.post { invalidateState() }
    }
  }
}
