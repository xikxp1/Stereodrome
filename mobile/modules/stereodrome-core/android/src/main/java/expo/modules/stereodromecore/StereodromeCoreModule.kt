package expo.modules.stereodromecore

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class StereodromeCoreModule : Module() {
  private val jni = StereodromeCoreJni()
  private var handle: Long = 0

  override fun definition() = ModuleDefinition {
    Name("StereodromeCore")

    AsyncFunction("initialize") { dataDir: String ->
      if (handle != 0L) {
        jni.destroy(handle)
      }
      handle = jni.initialize(dataDir)
      handle != 0L
    }

    AsyncFunction("getConnectionStatus") {
      callCore("getConnectionStatus", "null")
    }

    AsyncFunction("getStreamUri") { songId: String ->
      callCore("getStreamUri", "\"${escapeJson(songId)}\"")
    }

    AsyncFunction("call") { method: String, payload: String ->
      callCore(method, payload)
    }
  }

  private fun callCore(method: String, payload: String): String {
    if (handle == 0L) {
      return """{"ok":false,"error":"Stereodrome Rust core is not initialized"}"""
    }
    return jni.call(handle, method, payload)
  }

  private fun escapeJson(value: String): String =
    value.replace("\\", "\\\\").replace("\"", "\\\"")
}
