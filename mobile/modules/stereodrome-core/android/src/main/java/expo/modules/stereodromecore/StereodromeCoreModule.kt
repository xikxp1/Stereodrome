package expo.modules.stereodromecore

import android.os.Handler
import android.os.Looper
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class StereodromeCoreModule : Module() {
  private val mainHandler = Handler(Looper.getMainLooper())
  private var resourceDiagnostics: ResourceDiagnosticsCollector? = null

  override fun definition() = ModuleDefinition {
    Name("StereodromeCore")
    Events("core-event")

    OnDestroy {
      resourceDiagnostics?.close()
      resourceDiagnostics = null
      StereodromeCoreBridge.setCoreEventListener(null)
      StereodromeCoreBridge.destroy()
    }

    AsyncFunction("initialize") { dataDir: String ->
      val context = appContext.reactContext?.applicationContext
      if (context == null) {
        false
      } else {
        if (resourceDiagnostics == null) {
          resourceDiagnostics = ResourceDiagnosticsCollector(context)
        }
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

    AsyncFunction("startResourceDiagnostics") {
      requireResourceDiagnostics().start()
    }

    AsyncFunction("stopResourceDiagnostics") {
      requireResourceDiagnostics().stop()
    }

    AsyncFunction("getResourceDiagnosticsStatus") {
      requireResourceDiagnostics().status()
    }

    AsyncFunction("exportResourceDiagnostics") { destinationPath: String ->
      requireResourceDiagnostics().export(destinationPath)
    }

    AsyncFunction("clearResourceDiagnostics") {
      requireResourceDiagnostics().clear()
    }
  }

  private fun requireResourceDiagnostics(): ResourceDiagnosticsCollector =
    resourceDiagnostics ?: run {
      val context = appContext.reactContext?.applicationContext
        ?: throw IllegalStateException("Android application context is unavailable")
      ResourceDiagnosticsCollector(context).also { resourceDiagnostics = it }
    }
}
