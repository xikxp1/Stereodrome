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
  private var serviceStartPending = false

  private data class ProjectionUpdate(
    val player: StereodromeMediaPlayer?,
    val shouldStartService: Boolean,
  )

  private data class ClearUpdate(
    val player: StereodromeMediaPlayer?,
    val shouldStopService: Boolean,
  )

  fun attachService(
    service: StereodromeMediaSessionService,
    player: StereodromeMediaPlayer,
  ) {
    val info = synchronized(lock) {
      serviceStartPending = false
      this.service = WeakReference(service)
      this.player = player
      nowPlayingInfo
    }
    player.setNowPlayingInfo(info)
    if (info == null) {
      service.stopSelf()
    }
  }

  fun detachService(service: StereodromeMediaSessionService) {
    val shouldRestart = synchronized(lock) {
      if (this.service?.get() !== service) {
        return@synchronized false
      }
      this.service = null
      this.player = null
      if (nowPlayingInfo != null && !serviceStartPending) {
        serviceStartPending = true
        true
      } else {
        false
      }
    }
    if (shouldRestart) {
      startPendingService(service.applicationContext)
    }
  }

  fun applyPlaybackSnapshot(context: Context, snapshot: String) {
    val info = when (val projection = NowPlayingInfo.fromSnapshotJson(snapshot)) {
      NowPlayingProjection.Invalid -> return
      NowPlayingProjection.Stopped -> {
        clear(context, force = false)
        return
      }
      is NowPlayingProjection.Active -> projection.info
    }

    val update = synchronized(lock) {
      val currentService = service?.get()
      val currentPlayer = player
      if (
        currentService != null &&
        currentPlayer?.hasProjection(info) == true &&
        hasSameNowPlayingProjection(nowPlayingInfo, info)
      ) {
        return@synchronized null
      }
      nowPlayingInfo = info
      val shouldStartService = currentService == null && !serviceStartPending
      if (shouldStartService) {
        serviceStartPending = true
      }
      ProjectionUpdate(currentPlayer, shouldStartService)
    } ?: return
    if (update.shouldStartService) {
      startPendingService(context)
    }
    update.player?.setNowPlayingInfo(info)
  }

  fun clear(context: Context) = clear(context, force = true)

  private fun clear(context: Context, force: Boolean) {
    val update = synchronized(lock) {
      val currentService = service?.get()
      val currentPlayer = player
      val playerNeedsUpdate = currentPlayer?.hasProjection(null) == false
      val startWasPending = serviceStartPending
      serviceStartPending = false
      val shouldStopService = force || currentService != null || startWasPending
      if (nowPlayingInfo == null && !playerNeedsUpdate && !shouldStopService) {
        return@synchronized null
      }
      nowPlayingInfo = null
      ClearUpdate(
        player = currentPlayer?.takeIf { playerNeedsUpdate },
        shouldStopService = shouldStopService,
      )
    } ?: return
    update.player?.clearNowPlayingInfo()
    if (update.shouldStopService) {
      context.stopService(Intent(context, StereodromeMediaSessionService::class.java))
    }
  }

  private fun startService(context: Context, foreground: Boolean = false): Boolean {
    val intent = Intent(context, StereodromeMediaSessionService::class.java)
    try {
      if (foreground && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        context.startForegroundService(intent)
      } else {
        context.startService(intent)
      }
      return true
    } catch (exception: IllegalStateException) {
      if (
        !foreground &&
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
        exception.javaClass.name == BACKGROUND_SERVICE_START_NOT_ALLOWED
      ) {
        Log.w(TAG, "Skipped paused media session service start while app is backgrounded")
        return false
      } else {
        throw exception
      }
    }
  }

  private fun startPendingService(context: Context) {
    val foreground = synchronized(lock) { nowPlayingInfo?.isPlaying == true }
    try {
      if (!startService(context, foreground)) {
        synchronized(lock) {
          serviceStartPending = false
        }
        return
      }
    } catch (error: Throwable) {
      synchronized(lock) {
        serviceStartPending = false
      }
      throw error
    }
    val shouldStop = synchronized(lock) {
      if (nowPlayingInfo == null) {
        serviceStartPending = false
        true
      } else {
        false
      }
    }
    if (shouldStop) {
      context.stopService(Intent(context, StereodromeMediaSessionService::class.java))
    }
  }
}
