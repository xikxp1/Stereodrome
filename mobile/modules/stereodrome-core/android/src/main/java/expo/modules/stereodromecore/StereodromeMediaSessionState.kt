package expo.modules.stereodromecore

import android.content.Context
import android.content.Intent
import android.os.Build

object StereodromeMediaSessionState {
  private val lock = Any()
  private var player: StereodromeMediaPlayer? = null
  private var nowPlayingInfo: NowPlayingInfo? = null
  private var serviceStarted = false

  fun attachPlayer(player: StereodromeMediaPlayer) = synchronized(lock) {
    this.player = player
    player.setNowPlayingInfo(nowPlayingInfo)
  }

  fun detachPlayer(player: StereodromeMediaPlayer) = synchronized(lock) {
    if (this.player == player) {
      this.player = null
    }
  }

  fun setNowPlayingInfo(context: Context, info: NowPlayingInfo) = synchronized(lock) {
    nowPlayingInfo = info
    startService(context, foreground = info.isPlaying)
    player?.setNowPlayingInfo(info)
  }

  fun updateProgress(context: Context, progress: NowPlayingProgress) = synchronized(lock) {
    val current = nowPlayingInfo
    if (current != null && (progress.songId == null || progress.songId == current.songId)) {
      nowPlayingInfo = current.copy(
        durationSeconds = progress.durationSeconds,
        positionSeconds = progress.positionSeconds,
        isPlaying = progress.isPlaying,
      )
    }
    if (progress.isPlaying && !serviceStarted) {
      startService(context, foreground = true)
    }
    player?.updateProgress(progress)
  }

  fun clear(context: Context) = synchronized(lock) {
    nowPlayingInfo = null
    player?.clearNowPlayingInfo()
    context.stopService(Intent(context, StereodromeMediaSessionService::class.java))
    serviceStarted = false
  }

  private fun startService(context: Context, foreground: Boolean = false) {
    val intent = Intent(context, StereodromeMediaSessionService::class.java)
    if (foreground && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      context.startForegroundService(intent)
    } else {
      context.startService(intent)
    }
    serviceStarted = true
  }
}
