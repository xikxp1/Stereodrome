package expo.modules.stereodromecore

import android.os.Looper
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService

class StereodromeMediaSessionService : MediaSessionService() {
  private var mediaSession: MediaSession? = null
  private var player: StereodromeMediaPlayer? = null

  override fun onCreate() {
    super.onCreate()
    val mediaPlayer = StereodromeMediaPlayer(this, Looper.getMainLooper())
    player = mediaPlayer
    StereodromeMediaSessionState.attachPlayer(mediaPlayer)
    mediaSession = MediaSession.Builder(this, mediaPlayer).build()
  }

  override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? =
    mediaSession

  override fun onDestroy() {
    player?.release()
    mediaSession?.release()
    player?.let { StereodromeMediaSessionState.detachPlayer(it) }
    player = null
    mediaSession = null
    super.onDestroy()
  }
}
