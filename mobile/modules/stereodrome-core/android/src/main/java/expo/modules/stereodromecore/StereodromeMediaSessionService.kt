package expo.modules.stereodromecore

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioManager
import android.os.Build
import android.os.Looper
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService

class StereodromeMediaSessionService : MediaSessionService() {
  private var mediaSession: MediaSession? = null
  private var player: StereodromeMediaPlayer? = null

  private val becomingNoisyReceiver = object : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
      if (intent?.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) {
        StereodromeCoreCommandQueue.enqueue("audioPause") {
          StereodromeCoreBridge.pause()
        }
      }
    }
  }

  override fun onCreate() {
    super.onCreate()
    val mediaPlayer = StereodromeMediaPlayer(this, Looper.getMainLooper())
    player = mediaPlayer
    StereodromeMediaSessionState.attachService(this, mediaPlayer)
    mediaSession = MediaSession.Builder(this, mediaPlayer).build()
    val filter = IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      registerReceiver(becomingNoisyReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
    } else {
      registerReceiver(becomingNoisyReceiver, filter)
    }
  }

  override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? =
    mediaSession

  override fun onDestroy() {
    unregisterReceiver(becomingNoisyReceiver)
    StereodromeMediaSessionState.detachService(this)
    player?.release()
    mediaSession?.release()
    player = null
    mediaSession = null
    super.onDestroy()
  }
}
