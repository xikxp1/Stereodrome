package expo.modules.stereodromecore

import android.util.Log
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
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

  fun enqueueCommand(commandName: String, block: () -> String): ListenableFuture<Any> {
    val future = SettableFuture.create<Any>()
    executor.execute {
      try {
        val response = block()
        val error = StereodromeCoreBridge.runtimeResponseError(response)
        if (error == null) {
          future.set(Any())
        } else {
          Log.w(TAG, "Core command failed: $commandName: $error")
          future.setException(IllegalStateException(error))
        }
      } catch (error: Throwable) {
        Log.e(TAG, "Core command failed: $commandName", error)
        future.setException(error)
      }
    }
    return future
  }
}
