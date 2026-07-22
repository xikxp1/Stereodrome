package expo.modules.stereodromecore

import android.os.Handler
import android.os.Looper
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class StereodromeCoreModule : Module() {
  private val mainHandler = Handler(Looper.getMainLooper())

  override fun definition() = ModuleDefinition {
    Name("StereodromeCore")
    Events("core-event")

    OnDestroy {
      StereodromeCoreBridge.setCoreEventListener(null)
      StereodromeCoreBridge.destroy()
    }

    AsyncFunction("initialize") { dataDir: String ->
      val context = appContext.reactContext?.applicationContext
      if (context == null) {
        false
      } else {
        StereodromeCoreBridge.setCoreEventListener { event ->
          mainHandler.post {
            sendEvent("core-event", mapOf("event" to event))
          }
        }
        StereodromeCoreBridge.initialize(context, dataDir)
      }
    }

    AsyncFunction("dispatch") { commandJson: String ->
      StereodromeCoreBridge.dispatch(commandJson)
    }
  }
}
