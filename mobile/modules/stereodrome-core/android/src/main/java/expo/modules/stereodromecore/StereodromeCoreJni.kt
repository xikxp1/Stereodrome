package expo.modules.stereodromecore

class StereodromeCoreJni {
  companion object {
    init {
      System.loadLibrary("stereodrome_core_jni")
    }
  }

  fun initialize(dataDir: String): Long = nativeInitialize(dataDir)

  fun destroy(handle: Long) {
    nativeDestroy(handle)
  }

  fun call(handle: Long, method: String, payload: String): String =
    nativeCall(handle, method, payload)

  fun dispatch(handle: Long, commandJson: String): String =
    nativeDispatch(handle, commandJson)

  private external fun nativeInitialize(dataDir: String): Long
  private external fun nativeDestroy(handle: Long)
  private external fun nativeCall(handle: Long, method: String, payload: String): String
  private external fun nativeDispatch(handle: Long, commandJson: String): String
}
