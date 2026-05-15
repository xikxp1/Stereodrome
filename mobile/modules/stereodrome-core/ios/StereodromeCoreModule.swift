import AVFoundation
import ExpoModulesCore

@_silgen_name("stereodrome_core_new")
private func stereodromeCoreNew(_ dataDir: UnsafePointer<CChar>) -> OpaquePointer?

@_silgen_name("stereodrome_core_destroy")
private func stereodromeCoreDestroy(_ core: OpaquePointer?)

@_silgen_name("stereodrome_core_call")
private func stereodromeCoreCall(
  _ core: OpaquePointer?,
  _ method: UnsafePointer<CChar>,
  _ payload: UnsafePointer<CChar>
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("stereodrome_core_free_string")
private func stereodromeCoreFreeString(_ value: UnsafeMutablePointer<CChar>?)

private typealias StereodromeRustLogCallback = @convention(c) (UnsafePointer<CChar>?) -> Void

@_silgen_name("stereodrome_core_set_log_callback")
private func stereodromeCoreSetLogCallback(_ callback: StereodromeRustLogCallback?)

private weak var activeStereodromeCoreModule: StereodromeCoreModule?

private func stereodromeRustLogCallback(_ message: UnsafePointer<CChar>?) {
  guard let message else {
    return
  }
  let logMessage = String(cString: message)
  NSLog("%@", logMessage)
  DispatchQueue.main.async {
    activeStereodromeCoreModule?.emitRustLog(logMessage)
  }
}

public class StereodromeCoreModule: Module {
  private var core: OpaquePointer?

  deinit {
    stereodromeCoreDestroy(core)
  }

  public func definition() -> ModuleDefinition {
    Name("StereodromeCore")

    AsyncFunction("initialize") { (_ dataDir: String) -> Bool in
      activeStereodromeCoreModule = self
      stereodromeCoreSetLogCallback(stereodromeRustLogCallback)
      self.configureAudioSession()
      if let existing = self.core {
        stereodromeCoreDestroy(existing)
      }
      self.core = dataDir.withCString { stereodromeCoreNew($0) }
      return self.core != nil
    }

    AsyncFunction("call") { (_ method: String, _ payload: String) -> String in
      guard let core = self.core else {
        return #"{"ok":false,"error":"Stereodrome Rust core is not initialized"}"#
      }

      return method.withCString { methodPointer in
        payload.withCString { payloadPointer in
          guard let resultPointer = stereodromeCoreCall(core, methodPointer, payloadPointer) else {
            return #"{"ok":false,"error":"Rust returned null"}"#
          }

          let result = String(cString: resultPointer)
          stereodromeCoreFreeString(resultPointer)
          return result
        }
      }
    }

    AsyncFunction("getConnectionStatus") { () -> String in
      return self.callSync(method: "getConnectionStatus", payload: "null")
    }

    AsyncFunction("getStreamUri") { (_ songId: String) -> String in
      let escapedSongId = songId
        .replacingOccurrences(of: "\\", with: "\\\\")
        .replacingOccurrences(of: "\"", with: "\\\"")
      return self.callSync(method: "getStreamUri", payload: "\"\(escapedSongId)\"")
    }
  }

  fileprivate func emitRustLog(_ message: String) {
    appContext?.jsLogger.info(message)
  }

  private func callSync(method: String, payload: String) -> String {
    guard let core else {
      return #"{"ok":false,"error":"Stereodrome Rust core is not initialized"}"#
    }

    return method.withCString { methodPointer in
      payload.withCString { payloadPointer in
        guard let resultPointer = stereodromeCoreCall(core, methodPointer, payloadPointer) else {
          return #"{"ok":false,"error":"Rust returned null"}"#
        }

        let result = String(cString: resultPointer)
        stereodromeCoreFreeString(resultPointer)
        return result
      }
    }
  }

  private func configureAudioSession() {
    let session = AVAudioSession.sharedInstance()
    do {
      try session.setCategory(.playback, mode: .default)
      try session.setActive(true)
    } catch {
      // Rust returns playback errors through the FFI call path; session setup
      // failure should not prevent core initialization in development builds.
    }
  }
}
