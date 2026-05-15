package expo.modules.stereodromecore

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class StereodromeCoreModule : Module() {
  override fun definition() = ModuleDefinition {
    Name("StereodromeCore")
    Events("native-playback-invalidated")

    AsyncFunction("initialize") { dataDir: String ->
      StereodromeCoreBridge.setInvalidationListener {
        sendEvent("native-playback-invalidated")
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
      if (method == "audioPause" || method == "audioStop") {
        abandonAudioFocus()
      }
      callCore(method, payload)
    }

    AsyncFunction("setNowPlayingInfo") { payload: Map<String, Any?> ->
      val context = appContext.reactContext ?: return@AsyncFunction
      StereodromeMediaSessionState.setNowPlayingInfo(
        context,
        NowPlayingInfo.fromPayload(payload),
      )
    }

    AsyncFunction("updateNowPlayingProgress") { payload: Map<String, Any?> ->
      val context = appContext.reactContext ?: return@AsyncFunction
      StereodromeMediaSessionState.updateProgress(
        context,
        NowPlayingProgress.fromPayload(payload),
      )
    }

    AsyncFunction("clearNowPlayingInfo") {
      val context = appContext.reactContext ?: return@AsyncFunction
      StereodromeMediaSessionState.clear(context)
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
