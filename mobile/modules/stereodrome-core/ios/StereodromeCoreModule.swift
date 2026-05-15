import AVFoundation
import ExpoModulesCore
import MediaPlayer
import UIKit

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
  private let coreQueue = DispatchQueue(label: "dev.xikxp1.stereodrome.mobile.core")
  private var remoteCommandTargets: [Any] = []

  deinit {
    clearRemoteCommandHandlers()
    coreQueue.sync {
      stereodromeCoreDestroy(core)
    }
  }

  public func definition() -> ModuleDefinition {
    Name("StereodromeCore")

    AsyncFunction("initialize") { (_ dataDir: String) -> Bool in
      activeStereodromeCoreModule = self
      stereodromeCoreSetLogCallback(stereodromeRustLogCallback)
      self.configureAudioSession()
      if let existing = self.core {
        self.coreQueue.sync {
          stereodromeCoreDestroy(existing)
        }
      }
      self.core = self.coreQueue.sync {
        dataDir.withCString { stereodromeCoreNew($0) }
      }
      self.configureRemoteCommandCenter()
      return self.core != nil
    }

    AsyncFunction("call") { (_ method: String, _ payload: String) -> String in
      return self.callSync(method: method, payload: payload)
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

    AsyncFunction("setNowPlayingInfo") { (_ payload: [String: Any]) in
      self.setNowPlayingInfo(payload)
    }

    AsyncFunction("updateNowPlayingProgress") { (_ payload: [String: Any]) in
      self.updateNowPlayingProgress(payload)
    }

    AsyncFunction("clearNowPlayingInfo") {
      self.clearNowPlayingInfo()
    }

    Events("native-playback-invalidated")
  }

  fileprivate func emitRustLog(_ message: String) {
    appContext?.jsLogger.info(message)
  }

  private func callSync(method: String, payload: String) -> String {
    return coreQueue.sync {
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

  private func setNowPlayingInfo(_ payload: [String: Any]) {
    var info: [String: Any] = [
      MPMediaItemPropertyTitle: stringValue(payload["title"]) ?? "Unknown Title",
      MPMediaItemPropertyArtist: stringValue(payload["artist"]) ?? "Unknown Artist",
      MPMediaItemPropertyAlbumTitle: stringValue(payload["album"]) ?? "Unknown Album",
      MPMediaItemPropertyPlaybackDuration: doubleValue(payload["duration_seconds"]),
      MPNowPlayingInfoPropertyElapsedPlaybackTime: doubleValue(payload["position_seconds"]),
      MPNowPlayingInfoPropertyPlaybackRate: boolValue(payload["is_playing"]) ? 1.0 : 0.0,
      MPNowPlayingInfoPropertyPlaybackQueueCount: intValue(payload["queue_count"]) ?? 0,
    ]

    if let queueIndex = intValue(payload["queue_index"]) {
      info[MPNowPlayingInfoPropertyPlaybackQueueIndex] = queueIndex
    }

    if let artwork = artworkValue(stringValue(payload["artwork_uri"])) {
      info[MPMediaItemPropertyArtwork] = artwork
    }

    MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    configureCommandAvailability(payload)
  }

  private func updateNowPlayingProgress(_ payload: [String: Any]) {
    var info = MPNowPlayingInfoCenter.default().nowPlayingInfo ?? [:]
    info[MPMediaItemPropertyPlaybackDuration] = doubleValue(payload["duration_seconds"])
    info[MPNowPlayingInfoPropertyElapsedPlaybackTime] = doubleValue(payload["position_seconds"])
    info[MPNowPlayingInfoPropertyPlaybackRate] = boolValue(payload["is_playing"]) ? 1.0 : 0.0
    MPNowPlayingInfoCenter.default().nowPlayingInfo = info
  }

  private func clearNowPlayingInfo() {
    MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
    let commandCenter = MPRemoteCommandCenter.shared()
    commandCenter.nextTrackCommand.isEnabled = false
    commandCenter.previousTrackCommand.isEnabled = false
    commandCenter.changePlaybackPositionCommand.isEnabled = false
    commandCenter.playCommand.isEnabled = false
    commandCenter.pauseCommand.isEnabled = false
    commandCenter.togglePlayPauseCommand.isEnabled = false
    commandCenter.stopCommand.isEnabled = false
  }

  private func configureRemoteCommandCenter() {
    clearRemoteCommandHandlers()
    let commandCenter = MPRemoteCommandCenter.shared()

    remoteCommandTargets.append(commandCenter.playCommand.addTarget { [weak self] _ in
      self?.performRemoteCommand(.play) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.pauseCommand.addTarget { [weak self] _ in
      self?.performRemoteCommand(.pause) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.togglePlayPauseCommand.addTarget { [weak self] _ in
      self?.performRemoteCommand(.toggle) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.nextTrackCommand.addTarget { [weak self] _ in
      self?.performRemoteCommand(.next) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.previousTrackCommand.addTarget { [weak self] _ in
      self?.performRemoteCommand(.previous) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.stopCommand.addTarget { [weak self] _ in
      self?.performRemoteCommand(.stop) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.changePlaybackPositionCommand.addTarget {
      [weak self] event in
      guard let event = event as? MPChangePlaybackPositionCommandEvent else {
        return .commandFailed
      }
      return self?.performRemoteSeek(event.positionTime) ?? .commandFailed
    })
  }

  private func clearRemoteCommandHandlers() {
    let commandCenter = MPRemoteCommandCenter.shared()
    for target in remoteCommandTargets {
      commandCenter.playCommand.removeTarget(target)
      commandCenter.pauseCommand.removeTarget(target)
      commandCenter.togglePlayPauseCommand.removeTarget(target)
      commandCenter.nextTrackCommand.removeTarget(target)
      commandCenter.previousTrackCommand.removeTarget(target)
      commandCenter.stopCommand.removeTarget(target)
      commandCenter.changePlaybackPositionCommand.removeTarget(target)
    }
    remoteCommandTargets.removeAll()
  }

  private enum RemoteCommandAction {
    case play
    case pause
    case toggle
    case next
    case previous
    case stop
  }

  private func performRemoteCommand(_ action: RemoteCommandAction) -> MPRemoteCommandHandlerStatus {
    switch action {
    case .play:
      let status = parseOkValue(callSync(method: "audioGetStatus", payload: "null"))
      if stringValue(status?["current_song_id"]) == nil {
        _ = callSync(method: "audioPlayCurrent", payload: "null")
      } else {
        _ = callSync(method: "audioResume", payload: "null")
      }
    case .pause:
      _ = callSync(method: "audioPause", payload: "null")
    case .toggle:
      let status = parseOkValue(callSync(method: "audioGetStatus", payload: "null"))
      if boolValue(status?["is_playing"]) {
        _ = callSync(method: "audioPause", payload: "null")
      } else if stringValue(status?["current_song_id"]) == nil {
        _ = callSync(method: "audioPlayCurrent", payload: "null")
      } else {
        _ = callSync(method: "audioResume", payload: "null")
      }
    case .next:
      _ = callSync(method: "playNext", payload: "true")
      _ = callSync(method: "audioPlayCurrent", payload: "null")
    case .previous:
      _ = callSync(method: "playPrevious", payload: "null")
      _ = callSync(method: "audioPlayCurrent", payload: "null")
    case .stop:
      _ = callSync(method: "audioStop", payload: "null")
    }

    DispatchQueue.main.async {
      self.sendEvent("native-playback-invalidated")
    }
    return .success
  }

  private func performRemoteSeek(_ positionSeconds: TimeInterval) -> MPRemoteCommandHandlerStatus {
    _ = callSync(method: "audioSeek", payload: "\(max(0.0, positionSeconds))")
    DispatchQueue.main.async {
      self.sendEvent("native-playback-invalidated")
    }
    return .success
  }

  private func configureCommandAvailability(_ payload: [String: Any]) {
    let commandCenter = MPRemoteCommandCenter.shared()
    commandCenter.nextTrackCommand.isEnabled = boolValue(payload["can_next"])
    commandCenter.previousTrackCommand.isEnabled = boolValue(payload["can_previous"])
    commandCenter.changePlaybackPositionCommand.isEnabled = boolValue(payload["can_seek"])
    commandCenter.playCommand.isEnabled = true
    commandCenter.pauseCommand.isEnabled = true
    commandCenter.togglePlayPauseCommand.isEnabled = true
    commandCenter.stopCommand.isEnabled = true
  }

  private func artworkValue(_ uri: String?) -> MPMediaItemArtwork? {
    guard let uri else {
      return nil
    }

    let url: URL?
    if uri.hasPrefix("file://") {
      url = URL(string: uri)
    } else {
      url = URL(fileURLWithPath: uri)
    }

    guard let url, let image = UIImage(contentsOfFile: url.path) else {
      return nil
    }

    return MPMediaItemArtwork(boundsSize: image.size) { _ in image }
  }

  private func parseOkValue(_ raw: String) -> [String: Any]? {
    guard
      let data = raw.data(using: .utf8),
      let envelope = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
      boolValue(envelope["ok"]),
      let value = envelope["value"] as? [String: Any]
    else {
      return nil
    }
    return value
  }

  private func stringValue(_ value: Any?) -> String? {
    if let value = value as? String, !value.isEmpty {
      return value
    }
    return nil
  }

  private func doubleValue(_ value: Any?) -> Double {
    if let value = value as? Double {
      return value
    }
    if let value = value as? NSNumber {
      return value.doubleValue
    }
    if let value = value as? String {
      return Double(value) ?? 0.0
    }
    return 0.0
  }

  private func intValue(_ value: Any?) -> Int? {
    if let value = value as? Int {
      return value
    }
    if let value = value as? NSNumber {
      return value.intValue
    }
    if let value = value as? String {
      return Int(value)
    }
    return nil
  }

  private func boolValue(_ value: Any?) -> Bool {
    if let value = value as? Bool {
      return value
    }
    if let value = value as? NSNumber {
      return value.boolValue
    }
    if let value = value as? String {
      return value == "true"
    }
    return false
  }
}
