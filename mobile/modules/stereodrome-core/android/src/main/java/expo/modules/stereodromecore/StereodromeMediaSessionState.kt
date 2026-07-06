package expo.modules.stereodromecore

import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log
import java.lang.ref.WeakReference

object StereodromeMediaSessionState {
  private const val TAG = "StereodromeMediaSession"
  private const val BACKGROUND_SERVICE_START_NOT_ALLOWED =
    "android.app.BackgroundServiceStartNotAllowedException"

  private val lock = Any()
  private var service: WeakReference<StereodromeMediaSessionService>? = null
  private var player: StereodromeMediaPlayer? = null
  private var nowPlayingInfo: NowPlayingInfo? = null

  fun attachService(
    service: StereodromeMediaSessionService,
    player: StereodromeMediaPlayer,
  ) = synchronized(lock) {
    this.service = WeakReference(service)
    this.player = player
    player.setNowPlayingInfo(nowPlayingInfo)
  }

  fun detachService(service: StereodromeMediaSessionService) = synchronized(lock) {
    if (this.service?.get() === service) {
      this.service = null
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
    return service?.get() != null
  }

  private fun startService(context: Context, foreground: Boolean = false) {
    val intent = Intent(context, StereodromeMediaSessionService::class.java)
    try {
      if (foreground && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        context.startForegroundService(intent)
      } else {
        context.startService(intent)
      }
    } catch (exception: IllegalStateException) {
      if (
        !foreground &&
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
        exception.javaClass.name == BACKGROUND_SERVICE_START_NOT_ALLOWED
      ) {
        Log.w(TAG, "Skipped paused media session service start while app is backgrounded")
      } else {
        throw exception
      }
    }
  }
}
