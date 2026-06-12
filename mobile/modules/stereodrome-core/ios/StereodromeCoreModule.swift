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
  private let remoteCommandQueue = DispatchQueue(
    label: "dev.xikxp1.stereodrome.mobile.remote-commands")
  private var remoteCommandTargets: [Any] = []
  private var audioSessionObservers: [NSObjectProtocol] = []
  // Only accessed on remoteCommandQueue.
  private var shouldResumeAfterInterruption = false

  deinit {
    clearRemoteCommandHandlers()
    clearAudioSessionObservers()
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
      self.configureAudioSessionObservers()
      return self.core != nil
    }

    AsyncFunction("call") { (_ method: String, _ payload: String) -> String in
      if method == "audioPlayCurrent" || method == "audioResume" {
        self.setAudioSessionActive(true)
      }
      let result = self.callSync(method: method, payload: payload)
      if method == "audioStop" {
        self.setAudioSessionActive(false)
      }
      switch method {
      case "audioPause", "audioResume", "audioSeek", "audioPlayCurrent", "audioApplySettings":
        // Mirror the new state into the system widget right away; the JS
        // sync path may be deduped or suspended (e.g. locking the phone
        // immediately after pausing in-app).
        self.refreshNowPlayingPlaybackState()
      default:
        break
      }
      return result
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
    } catch {
      // Rust returns playback errors through the FFI call path; session setup
      // failure should not prevent core initialization in development builds.
    }
  }

  private func setAudioSessionActive(_ active: Bool) {
    let session = AVAudioSession.sharedInstance()
    do {
      if active {
        try session.setCategory(.playback, mode: .default)
      }
      let options: AVAudioSession.SetActiveOptions = active ? [] : [.notifyOthersOnDeactivation]
      try session.setActive(active, options: options)
    } catch {
      // Keep command handling best-effort; playback errors still flow through Rust.
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
    // Never create a bare now-playing entry from a progress-only update; full
    // metadata must be published through setNowPlayingInfo first.
    guard var info = MPNowPlayingInfoCenter.default().nowPlayingInfo, !info.isEmpty else {
      return
    }
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
      self?.enqueueRemoteCommand(.play) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.pauseCommand.addTarget { [weak self] _ in
      self?.enqueueRemoteCommand(.pause) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.togglePlayPauseCommand.addTarget { [weak self] _ in
      self?.enqueueRemoteCommand(.toggle) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.nextTrackCommand.addTarget { [weak self] _ in
      self?.enqueueRemoteCommand(.next) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.previousTrackCommand.addTarget { [weak self] _ in
      self?.enqueueRemoteCommand(.previous) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.stopCommand.addTarget { [weak self] _ in
      self?.enqueueRemoteCommand(.stop) ?? .commandFailed
    })
    remoteCommandTargets.append(commandCenter.changePlaybackPositionCommand.addTarget {
      [weak self] event in
      guard let self, let event = event as? MPChangePlaybackPositionCommandEvent else {
        return .commandFailed
      }
      return self.enqueueRemoteSeek(event.positionTime)
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

  private func configureAudioSessionObservers() {
    clearAudioSessionObservers()
    let center = NotificationCenter.default
    let session = AVAudioSession.sharedInstance()

    audioSessionObservers.append(
      center.addObserver(
        forName: AVAudioSession.interruptionNotification, object: session, queue: nil
      ) { [weak self] notification in
        self?.handleAudioSessionInterruption(notification)
      })
    audioSessionObservers.append(
      center.addObserver(
        forName: AVAudioSession.routeChangeNotification, object: session, queue: nil
      ) { [weak self] notification in
        self?.handleAudioRouteChange(notification)
      })
    audioSessionObservers.append(
      center.addObserver(
        forName: AVAudioSession.mediaServicesWereResetNotification, object: session, queue: nil
      ) { [weak self] _ in
        self?.handleMediaServicesReset()
      })
  }

  private func clearAudioSessionObservers() {
    let center = NotificationCenter.default
    for observer in audioSessionObservers {
      center.removeObserver(observer)
    }
    audioSessionObservers.removeAll()
  }

  private func handleAudioSessionInterruption(_ notification: Notification) {
    guard
      let typeValue = notification.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
      let type = AVAudioSession.InterruptionType(rawValue: typeValue)
    else {
      return
    }

    switch type {
    case .began:
      // iOS re-posts a queued interruption when the app wakes from suspension
      // (reason .appWasSuspended). Nothing was playing when it happened, and
      // it can be delivered right after a lock-screen play command — treating
      // it as live would pause the track the user just resumed.
      if let reasonValue = notification.userInfo?[AVAudioSessionInterruptionReasonKey] as? UInt,
        let reason = AVAudioSession.InterruptionReason(rawValue: reasonValue),
        reason == .appWasSuspended
      {
        return
      }
      // iOS has halted our audio output. Pause the Rust core so its state
      // matches reality; otherwise a later remote play command becomes a
      // no-op (resume on a sink that was never paused).
      remoteCommandQueue.async {
        let wasPlaying = self.isCorePlaying()
        self.shouldResumeAfterInterruption = wasPlaying
        if wasPlaying {
          _ = self.callSync(method: "audioPause", payload: "null")
        }
        self.notifyNativePlaybackChanged()
      }
    case .ended:
      let optionsValue =
        notification.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
      let options = AVAudioSession.InterruptionOptions(rawValue: optionsValue)
      remoteCommandQueue.async {
        let shouldResume =
          self.shouldResumeAfterInterruption && options.contains(.shouldResume)
        self.shouldResumeAfterInterruption = false
        if shouldResume {
          self.setAudioSessionActive(true)
          _ = self.callSync(method: "audioResume", payload: "null")
        }
        self.notifyNativePlaybackChanged()
      }
    @unknown default:
      break
    }
  }

  private func handleAudioRouteChange(_ notification: Notification) {
    guard
      let reasonValue = notification.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
      let reason = AVAudioSession.RouteChangeReason(rawValue: reasonValue)
    else {
      return
    }

    // Headphones unplugged / Bluetooth device disconnected: pause, matching
    // platform convention and keeping Rust state in sync with the halted output.
    guard reason == .oldDeviceUnavailable else {
      return
    }

    remoteCommandQueue.async {
      self.shouldResumeAfterInterruption = false
      if self.isCorePlaying() {
        _ = self.callSync(method: "audioPause", payload: "null")
        self.notifyNativePlaybackChanged()
      }
    }
  }

  private func handleMediaServicesReset() {
    remoteCommandQueue.async {
      self.shouldResumeAfterInterruption = false
      _ = self.callSync(method: "audioPause", payload: "null")
      self.configureAudioSession()
      self.notifyNativePlaybackChanged()
    }
  }

  private func isCorePlaying() -> Bool {
    let status = parseOkValue(callSync(method: "audioGetStatus", payload: "null"))
    return boolValue(status?["is_playing"])
  }

  /// Pushes the Rust core's playback state into the system now-playing info and
  /// notifies JS. Keeps the lock screen accurate even when the JS runtime is
  /// suspended in the background.
  private func notifyNativePlaybackChanged() {
    refreshNowPlayingPlaybackState()
    DispatchQueue.main.async {
      self.sendEvent("native-playback-invalidated")
    }
  }

  private func refreshNowPlayingPlaybackState() {
    guard
      let status = parseOkValue(callSync(method: "audioGetStatus", payload: "null")),
      stringValue(status["current_song_id"]) != nil
    else {
      return
    }
    guard var info = MPNowPlayingInfoCenter.default().nowPlayingInfo, !info.isEmpty else {
      return
    }
    let duration = doubleValue(status["duration"])
    if duration > 0 {
      info[MPMediaItemPropertyPlaybackDuration] = duration
    }
    info[MPNowPlayingInfoPropertyElapsedPlaybackTime] = doubleValue(status["position"])
    info[MPNowPlayingInfoPropertyPlaybackRate] = boolValue(status["is_playing"]) ? 1.0 : 0.0
    MPNowPlayingInfoCenter.default().nowPlayingInfo = info
  }

  private enum RemoteCommandAction {
    case play
    case pause
    case toggle
    case next
    case previous
    case stop
  }

  /// Remote command handlers run on the main thread, but the underlying core
  /// calls can block (e.g. audioPlayCurrent downloads the song). Acknowledge
  /// the command immediately and run it on a serial background queue.
  private func enqueueRemoteCommand(_ action: RemoteCommandAction) -> MPRemoteCommandHandlerStatus {
    remoteCommandQueue.async {
      self.performRemoteCommand(action)
    }
    return .success
  }

  private func enqueueRemoteSeek(_ positionSeconds: TimeInterval) -> MPRemoteCommandHandlerStatus {
    remoteCommandQueue.async {
      _ = self.callSync(method: "audioSeek", payload: "\(max(0.0, positionSeconds))")
      self.notifyNativePlaybackChanged()
    }
    return .success
  }

  private func performRemoteCommand(_ action: RemoteCommandAction) {
    switch action {
    case .play:
      setAudioSessionActive(true)
      let status = parseOkValue(callSync(method: "audioGetStatus", payload: "null"))
      if stringValue(status?["current_song_id"]) == nil {
        playCurrentFromPersistedPosition()
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
        setAudioSessionActive(true)
        playCurrentFromPersistedPosition()
      } else {
        setAudioSessionActive(true)
        _ = callSync(method: "audioResume", payload: "null")
      }
    case .next:
      setAudioSessionActive(true)
      _ = callSync(method: "playNext", payload: "true")
      _ = callSync(method: "audioPlayCurrent", payload: "null")
    case .previous:
      setAudioSessionActive(true)
      _ = callSync(method: "playPrevious", payload: "null")
      _ = callSync(method: "audioPlayCurrent", payload: "null")
    case .stop:
      _ = callSync(method: "audioStop", payload: "null")
      setAudioSessionActive(false)
    }

    notifyNativePlaybackChanged()
  }

  /// Starting playback from a stopped core (e.g. lock-screen play after the
  /// app restored a paused session) should continue from the persisted
  /// position instead of restarting the song from the beginning.
  private func playCurrentFromPersistedPosition() {
    let snapshot = parseOkValue(callSync(method: "getPlaybackState", payload: "null"))
    let savedSongId = stringValue(snapshot?["current_song_id"])
    let savedPosition = doubleValue(snapshot?["position_seconds"])

    let status = parseOkValue(callSync(method: "audioPlayCurrent", payload: "null"))
    guard
      let playingSongId = stringValue(status?["current_song_id"]),
      playingSongId == savedSongId,
      savedPosition > 0.5
    else {
      return
    }

    let duration = doubleValue(status?["duration"])
    let target = duration > 0 ? min(savedPosition, max(0.0, duration - 1.0)) : savedPosition
    _ = callSync(method: "audioSeek", payload: "\(target)")
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
