package expo.modules.stereodromecore

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build

object StereodromeAudioFocus {
  class Lease internal constructor(val acquiredNow: Boolean)

  private val lock = Any()
  private var focusRequest: AudioFocusRequest? = null
  private var ownsFocus = false
  private var focusGeneration = 0L
  @Volatile private var shouldResumeAfterTransientLoss = false
  private val focusChangeListener = AudioManager.OnAudioFocusChangeListener { focusChange ->
    handleFocusChange(focusChange)
  }

  fun request(context: Context): Lease? = synchronized(lock) {
    if (ownsFocus) {
      return@synchronized Lease(acquiredNow = false)
    }

    val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    val result = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      val request = focusRequest ?: AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
        .setAudioAttributes(
          AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_MEDIA)
            .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
            .build()
        )
        .setOnAudioFocusChangeListener(focusChangeListener)
        .build()
        .also { focusRequest = it }
      audioManager.requestAudioFocus(request)
    } else {
      @Suppress("DEPRECATION")
      audioManager.requestAudioFocus(
        focusChangeListener,
        AudioManager.STREAM_MUSIC,
        AudioManager.AUDIOFOCUS_GAIN
      )
    }
    if (result != AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
      return@synchronized null
    }
    ownsFocus = true
    focusGeneration += 1
    Lease(acquiredNow = true)
  }

  fun abandon(context: Context) = synchronized(lock) {
    shouldResumeAfterTransientLoss = false
    if (!ownsFocus) {
      return@synchronized
    }
    val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      focusRequest?.let { audioManager.abandonAudioFocusRequest(it) }
    } else {
      @Suppress("DEPRECATION")
      audioManager.abandonAudioFocus(focusChangeListener)
    }
    ownsFocus = false
    focusGeneration += 1
  }

  fun rollback(context: Context, lease: Lease) {
    if (lease.acquiredNow) {
      abandon(context)
    }
  }

  private fun handleFocusChange(focusChange: Int) {
    when (focusChange) {
      AudioManager.AUDIOFOCUS_LOSS -> {
        val lossGeneration = synchronized(lock) {
          ownsFocus = false
          focusGeneration += 1
          focusGeneration
        }
        StereodromeCoreCommandQueue.enqueue("audioFocusLoss") {
          if (!isCurrentGeneration(lossGeneration)) {
            return@enqueue
          }
          shouldResumeAfterTransientLoss = false
          StereodromeCoreBridge.pauseFromAudioFocusLoss()
        }
      }
      AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
        val lossGeneration = synchronized(lock) {
          ownsFocus = false
          focusGeneration += 1
          focusGeneration
        }
        StereodromeCoreCommandQueue.enqueue("audioFocusLossTransient") {
          if (!isCurrentGeneration(lossGeneration)) {
            return@enqueue
          }
          shouldResumeAfterTransientLoss =
            StereodromeCoreBridge.pauseFromTransientAudioFocusLoss()
        }
      }
      AudioManager.AUDIOFOCUS_GAIN -> {
        val gainGeneration = synchronized(lock) {
          ownsFocus = true
          focusGeneration += 1
          focusGeneration
        }
        StereodromeCoreCommandQueue.enqueue("audioFocusGain") {
          if (!isCurrentGeneration(gainGeneration)) {
            return@enqueue
          }
          val shouldResume = shouldResumeAfterTransientLoss
          shouldResumeAfterTransientLoss = false
          if (shouldResume) {
            StereodromeCoreBridge.resumeFromAudioFocusGain()
          }
        }
      }
    }
  }

  private fun isCurrentGeneration(expected: Long): Boolean = synchronized(lock) {
    focusGeneration == expected
  }
}
