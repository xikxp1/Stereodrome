package expo.modules.stereodromecore

import android.content.Context
import android.content.Intent
import android.os.Build

object StereodromeMediaSessionState {
  private val lock = Any()
  private var player: StereodromeMediaPlayer? = null
  private var nowPlayingInfo: NowPlayingInfo? = null

  fun attachPlayer(player: StereodromeMediaPlayer) = synchronized(lock) {
    this.player = player
    player.setNowPlayingInfo(nowPlayingInfo)
  }

  fun detachPlayer(player: StereodromeMediaPlayer) = synchronized(lock) {
    if (this.player == player) {
      this.player = null
    }
  }

  fun applyPlaybackSnapshot(context: Context, snapshot: String) = synchronized(lock) {
    val info = NowPlayingInfo.fromSnapshotJson(snapshot)
    if (info == null) {
      clearLocked(context)
      return@synchronized
    }

    nowPlayingInfo = info
    if (!isServiceRunningLocked()) {
      startService(context, foreground = info.isPlaying)
    }
    player?.setNowPlayingInfo(info)
  }

  fun clear(context: Context) = synchronized(lock) {
    clearLocked(context)
  }

  private fun clearLocked(context: Context) {
    nowPlayingInfo = null
    player?.clearNowPlayingInfo()
    context.stopService(Intent(context, StereodromeMediaSessionService::class.java))
  }

  private fun isServiceRunningLocked(): Boolean {
    return player != null
  }

  private fun startService(context: Context, foreground: Boolean = false) {
    val intent = Intent(context, StereodromeMediaSessionService::class.java)
    if (foreground && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      context.startForegroundService(intent)
    } else {
      context.startService(intent)
    }
  }
}
