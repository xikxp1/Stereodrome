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
import kotlin.math.max

class StereodromeMediaPlayer(
  context: Context,
  private val playerLooper: Looper,
) : SimpleBasePlayer(playerLooper) {
  private val appContext = context.applicationContext
  private val handler = Handler(playerLooper)
  private var info: NowPlayingInfo? = null

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
    info = nextInfo
    invalidateOnPlayerLooper()
  }

  fun updateProgress(progress: NowPlayingProgress) {
    val current = info ?: return
    if (progress.songId != null && progress.songId != current.songId) {
      return
    }
    info = current.copy(
      durationSeconds = progress.durationSeconds,
      positionSeconds = progress.positionSeconds,
      isPlaying = progress.isPlaying,
    )
    invalidateOnPlayerLooper()
  }

  fun clearNowPlayingInfo() {
    info = null
    invalidateOnPlayerLooper()
  }

  override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<Any> {
    if (playWhenReady) {
      if (info?.canPlay != true) {
        invalidateOnPlayerLooper()
        return Futures.immediateFuture(Any())
      }
      StereodromeAudioFocus.request(appContext)
      StereodromeCoreBridge.play()
    } else {
      StereodromeCoreBridge.pause()
      StereodromeAudioFocus.abandon(appContext)
    }
    info = info?.copy(isPlaying = playWhenReady)
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
        StereodromeAudioFocus.request(appContext)
        StereodromeCoreBridge.next()
      }
      Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> {
        if (info?.canPlay != true) {
          return Futures.immediateFuture(Any())
        }
        StereodromeAudioFocus.request(appContext)
        StereodromeCoreBridge.previous()
      }
      Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM -> {
        val nextPositionSeconds = max(0.0, positionMs / 1000.0)
        StereodromeCoreBridge.seekTo(nextPositionSeconds)
        info = info?.copy(positionSeconds = nextPositionSeconds)
      }
    }
    return Futures.immediateFuture(Any())
  }

  override fun handleStop(): ListenableFuture<Any> {
    StereodromeCoreBridge.stop()
    StereodromeAudioFocus.abandon(appContext)
    info = info?.copy(isPlaying = false, positionSeconds = 0.0)
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

  private fun invalidateOnPlayerLooper() {
    if (Looper.myLooper() == playerLooper) {
      invalidateState()
    } else {
      handler.post { invalidateState() }
    }
  }
}
