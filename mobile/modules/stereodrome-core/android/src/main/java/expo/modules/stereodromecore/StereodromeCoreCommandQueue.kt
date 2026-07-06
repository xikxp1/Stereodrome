package expo.modules.stereodromecore

import android.util.Log
import java.util.concurrent.Executors

object StereodromeCoreCommandQueue {
  private const val TAG = "StereodromeCoreCommandQueue"

  private val executor = Executors.newSingleThreadExecutor { runnable ->
    Thread(runnable, "StereodromeCoreCommandQueue").apply {
      isDaemon = true
    }
  }

  fun enqueue(commandName: String, block: () -> Unit) {
    executor.execute {
      try {
        block()
      } catch (error: Throwable) {
        Log.e(TAG, "Core command failed: $commandName", error)
      }
    }
  }
}
