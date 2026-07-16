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
  ) {
    val info = synchronized(lock) {
      this.service = WeakReference(service)
      this.player = player
      nowPlayingInfo
    }
    player.setNowPlayingInfo(info)
  }

  fun detachService(service: StereodromeMediaSessionService) = synchronized(lock) {
    if (this.service?.get() === service) {
      this.service = null
      this.player = null
    }
  }

  fun applyPlaybackSnapshot(context: Context, snapshot: String) {
    val info = NowPlayingInfo.fromSnapshotJson(snapshot)
    if (info == null) {
      clear(context)
      return
    }

    val (currentPlayer, shouldStartService) = synchronized(lock) {
      nowPlayingInfo = info
      player to (service?.get() == null)
    }
    if (shouldStartService) {
      startService(context, foreground = info.isPlaying)
    }
    currentPlayer?.setNowPlayingInfo(info)
  }

  fun clear(context: Context) {
    val currentPlayer = synchronized(lock) {
      nowPlayingInfo = null
      player
    }
    currentPlayer?.clearNowPlayingInfo()
    context.stopService(Intent(context, StereodromeMediaSessionService::class.java))
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
