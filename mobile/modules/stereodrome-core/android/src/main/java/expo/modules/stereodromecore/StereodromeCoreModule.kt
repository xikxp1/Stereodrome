package expo.modules.stereodromecore

import android.content.Context
import android.os.Handler
import android.os.Looper
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class StereodromeCoreModule : Module() {
  private val mainHandler = Handler(Looper.getMainLooper())
  private var applicationContext: Context? = null

  override fun definition() = ModuleDefinition {
    Name("StereodromeCore")
    Events("playback-snapshot")

    AsyncFunction("initialize") { dataDir: String ->
      applicationContext = appContext.reactContext?.applicationContext
      StereodromeCoreBridge.setPlaybackSnapshotListener { snapshot ->
        mainHandler.post {
          applicationContext?.let { context ->
            StereodromeMediaSessionState.applyPlaybackSnapshot(context, snapshot)
          }
          sendEvent("playback-snapshot", mapOf("snapshot" to snapshot))
        }
      }
      StereodromeCoreBridge.initialize(dataDir)
    }

    AsyncFunction("getConnectionStatus") {
      callCore("getConnectionStatus", "null")
    }

    AsyncFunction("getStreamUri") { songId: String ->
      callCore("getStreamUri", "\"${escapeJson(songId)}\"")
    }

    AsyncFunction("call") { method: String, payload: String ->
      if (method == "audioPlayCurrent" || method == "audioResume") {
        requestAudioFocus()
      }
      val result = callCore(method, payload)
      if (method == "audioStop") {
        abandonAudioFocus()
      }
      result
    }
  }

  private fun callCore(method: String, payload: String): String {
    return StereodromeCoreBridge.call(method, payload)
  }

  private fun escapeJson(value: String): String =
    value.replace("\\", "\\\\").replace("\"", "\\\"")

  private fun requestAudioFocus() {
    val context = appContext.reactContext ?: return
    StereodromeAudioFocus.request(context.applicationContext)
  }

  private fun abandonAudioFocus() {
    val context = appContext.reactContext ?: return
    StereodromeAudioFocus.abandon(context.applicationContext)
  }
}
