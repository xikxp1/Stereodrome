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
private typealias StereodromePlaybackCallback = @convention(c) (UnsafePointer<CChar>?) -> Void

@_silgen_name("stereodrome_core_set_log_callback")
private func stereodromeCoreSetLogCallback(_ callback: StereodromeRustLogCallback?)

@_silgen_name("stereodrome_core_set_playback_callback")
private func stereodromeCoreSetPlaybackCallback(_ callback: StereodromePlaybackCallback?)

private weak var activeStereodromeCoreModule: StereodromeCoreModule?
private let activeStereodromeCoreModuleLock = NSLock()

private func getActiveStereodromeCoreModule() -> StereodromeCoreModule? {
  activeStereodromeCoreModuleLock.lock()
  defer { activeStereodromeCoreModuleLock.unlock() }
  return activeStereodromeCoreModule
}

private func setActiveStereodromeCoreModule(_ module: StereodromeCoreModule) {
  activeStereodromeCoreModuleLock.lock()
  activeStereodromeCoreModule = module
  activeStereodromeCoreModuleLock.unlock()
}

private func clearActiveStereodromeCoreModule(_ module: StereodromeCoreModule) -> Bool {
  activeStereodromeCoreModuleLock.lock()
  defer { activeStereodromeCoreModuleLock.unlock() }
  guard activeStereodromeCoreModule === module else {
    return false
  }
  activeStereodromeCoreModule = nil
  return true
}


private func performOnMainSync<T>(_ action: () -> T) -> T {
  if Thread.isMainThread {
    return action()
  }
  return DispatchQueue.main.sync(execute: action)
}

private func clearSystemNowPlayingInfo() {
  let center = MPNowPlayingInfoCenter.default()
  let commandCenter = MPRemoteCommandCenter.shared()
  commandCenter.nextTrackCommand.isEnabled = false
  commandCenter.previousTrackCommand.isEnabled = false
  commandCenter.changePlaybackPositionCommand.isEnabled = false
  commandCenter.playCommand.isEnabled = false
  commandCenter.pauseCommand.isEnabled = false
  commandCenter.togglePlayPauseCommand.isEnabled = false
  commandCenter.stopCommand.isEnabled = false
  center.playbackState = .stopped
  // Clearing metadata must be the final MediaPlayer update. Publishing a playback
  // state after this can make iOS retain an empty now-playing widget.
  center.nowPlayingInfo = nil
}

private func stereodromeRustLogCallback(_ message: UnsafePointer<CChar>?) {
  guard let message else {
    return
  }
  let logMessage = String(cString: message)
  NSLog("%@", logMessage)
  DispatchQueue.main.async {
    getActiveStereodromeCoreModule()?.emitRustLog(logMessage)
  }
}

private func stereodromePlaybackCallback(_ snapshot: UnsafePointer<CChar>?) {
  guard let snapshot else {
    return
  }
  let rawSnapshot = String(cString: snapshot)
  // Holding the module through the synchronous hop prevents deinit from joining
  // a Rust monitor thread that is itself waiting for this callback on main.
  guard let module = getActiveStereodromeCoreModule() else {
    return
  }
  // Rust treats the snapshot callback return as transport completion. Keep the
  // scalar OS projection inside that boundary so suspension cannot preserve stale state.
  let artworkUri: String? = performOnMainSync {
    guard getActiveStereodromeCoreModule() === module else {
      return nil
    }
    return module.applyPlaybackSnapshot(rawSnapshot)
  }
  module.enqueueDeferredPlaybackSnapshotUpdates(rawSnapshot, artworkUri: artworkUri)
}

public class StereodromeCoreModule: Module {
  private var core: OpaquePointer?
  private let coreQueue = DispatchQueue(label: "dev.xikxp1.stereodrome.mobile.core")
  private let remoteCommandQueue = DispatchQueue(
    label: "dev.xikxp1.stereodrome.mobile.remote-commands")
  private let artworkCache = NSCache<NSString, MPMediaItemArtwork>()
  private let artworkQueue = DispatchQueue(
    label: "dev.xikxp1.stereodrome.mobile.artwork", qos: .utility)
  private var currentArtworkUri: String?
  private let remoteCommandStateLock = NSLock()
  private var remoteCommandTargets: [Any] = []
  private var audioSessionObservers: [NSObjectProtocol] = []
  private var shouldResumeAfterInterruption = false
  private var canPlayRemoteCommandsValue = false

  deinit {
    clearAudioSessionObservers()
    if clearActiveStereodromeCoreModule(self) {
      stereodromeCoreSetPlaybackCallback(nil)
      performOnMainSync {
        self.clearNowPlayingInfo()
      }
    } else {
      clearRemoteCommandHandlers()
    }
    coreQueue.sync {
      stereodromeCoreDestroy(core)
    }
  }

  public func definition() -> ModuleDefinition {
    Name("StereodromeCore")

    AsyncFunction("initialize") { (_ dataDir: String) -> Bool in
      setActiveStereodromeCoreModule(self)
      stereodromeCoreSetLogCallback(stereodromeRustLogCallback)
      stereodromeCoreSetPlaybackCallback(stereodromePlaybackCallback)
      self.configureAudioSession()
      if self.core == nil {
        self.core = self.coreQueue.sync {
          dataDir.withCString { stereodromeCoreNew($0) }
        }
      }
      self.configureAudioSessionObservers()
      return self.core != nil
    }

    AsyncFunction("call") { (_ method: String, _ payload: String) -> String in
      if method == "audioPlayCurrent" || method == "audioPlayQueueItem"
        || method == "audioPlayNext" || method == "audioPlayPrevious"
        || method == "audioResume" {
        self.setAudioSessionActive(true)
      }
      let result = self.callSync(method: method, payload: payload)
      if method == "audioStop" {
        self.setAudioSessionActive(false)
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

    Events("playback-snapshot")
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

  fileprivate func enqueueDeferredPlaybackSnapshotUpdates(
    _ snapshot: String,
    artworkUri: String?
  ) {
    DispatchQueue.main.async { [weak self] in
      guard let self, getActiveStereodromeCoreModule() === self else {
        return
      }
      self.sendEvent("playback-snapshot", ["snapshot": snapshot])
    }

    guard let artworkUri else {
      return
    }
    artworkQueue.async { [weak self] in
      guard let self, let artwork = self.artworkValue(artworkUri) else {
        return
      }
      DispatchQueue.main.async { [weak self] in
        guard
          let self,
          getActiveStereodromeCoreModule() === self,
          self.currentArtworkUri == artworkUri,
          var info = MPNowPlayingInfoCenter.default().nowPlayingInfo
        else {
          return
        }
        info[MPMediaItemPropertyArtwork] = artwork
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
      }
    }
  }

  fileprivate func applyPlaybackSnapshot(_ snapshotJson: String) -> String? {
    guard
      let data = snapshotJson.data(using: .utf8),
      let snapshot = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    else {
      return nil
    }

    guard
      stringValue(snapshot["state"]) != "stopped",
      let song = snapshot["song"] as? [String: Any]
    else {
      clearNowPlayingInfo()
      return nil
    }

    let duration = doubleValue(snapshot["duration_seconds"]) > 0
      ? doubleValue(snapshot["duration_seconds"])
      : doubleValue(song["duration_seconds"])
    var info: [String: Any] = [
      MPMediaItemPropertyTitle: stringValue(song["title"]) ?? "Unknown Title",
      MPMediaItemPropertyArtist: stringValue(song["artist"]) ?? "Unknown Artist",
      MPMediaItemPropertyAlbumTitle: stringValue(song["album"]) ?? "Unknown Album",
      MPMediaItemPropertyPlaybackDuration: duration,
      MPNowPlayingInfoPropertyElapsedPlaybackTime: doubleValue(snapshot["position_seconds"]),
      MPNowPlayingInfoPropertyPlaybackRate: boolValue(snapshot["is_playing"]) ? 1.0 : 0.0,
      MPNowPlayingInfoPropertyPlaybackQueueCount: intValue(snapshot["queue_length"]) ?? 0,
    ]

    if let queueIndex = intValue(snapshot["queue_index"]) {
      info[MPNowPlayingInfoPropertyPlaybackQueueIndex] = queueIndex
    }

    let artworkUri = stringValue(song["artwork_uri"])
    if let artworkUri,
      let artwork = artworkCache.object(forKey: artworkUri as NSString)
    {
      info[MPMediaItemPropertyArtwork] = artwork
    }

    currentArtworkUri = artworkUri
    let isPlaying = boolValue(snapshot["is_playing"])
    MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    updateNowPlayingPlaybackState(isPlaying: isPlaying)
    if remoteCommandTargets.isEmpty {
      configureRemoteCommandCenter()
    }
    configureCommandAvailability(snapshot)
    return artworkUri
  }

  private func clearNowPlayingInfo() {
    currentArtworkUri = nil
    setCanPlayRemoteCommands(false)
    let hasPublishedSession =
      MPNowPlayingInfoCenter.default().nowPlayingInfo != nil || !remoteCommandTargets.isEmpty
    guard hasPublishedSession else {
      return
    }
    clearSystemNowPlayingInfo()
    clearRemoteCommandHandlers()
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
      }
    }
  }

  private func handleMediaServicesReset() {
    remoteCommandQueue.async {
      self.shouldResumeAfterInterruption = false
      self.configureAudioSession()
      self.setAudioSessionActive(true)
      _ = self.callSync(method: "audioRebuildOutput", payload: "null")
    }
  }

  private func isCorePlaying() -> Bool {
    let snapshot = parseOkValue(callSync(method: "getPlaybackSnapshot", payload: "null"))
    return boolValue(snapshot?["is_playing"])
  }

  private func updateNowPlayingPlaybackState(isPlaying: Bool) {
    MPNowPlayingInfoCenter.default().playbackState = isPlaying ? .playing : .paused
  }

  private func canPlayRemoteCommands() -> Bool {
    remoteCommandStateLock.lock()
    defer { remoteCommandStateLock.unlock() }
    return canPlayRemoteCommandsValue
  }

  private func setCanPlayRemoteCommands(_ canPlay: Bool) {
    remoteCommandStateLock.lock()
    canPlayRemoteCommandsValue = canPlay
    remoteCommandStateLock.unlock()
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
  /// calls can block (e.g. audioPlayNext downloads the song). Acknowledge
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
    }
    return .success
  }

  private func performRemoteCommand(_ action: RemoteCommandAction) {
    switch action {
    case .play:
      guard canPlayRemoteCommands() else {
        return
      }
      setAudioSessionActive(true)
      _ = callSync(method: "audioResume", payload: "null")
    case .pause:
      _ = callSync(method: "audioPause", payload: "null")
    case .toggle:
      let snapshot = parseOkValue(callSync(method: "getPlaybackSnapshot", payload: "null"))
      if boolValue(snapshot?["is_playing"]) {
        _ = callSync(method: "audioPause", payload: "null")
      } else if !canPlayRemoteCommands() {
        return
      } else {
        setAudioSessionActive(true)
        _ = callSync(method: "audioResume", payload: "null")
      }
    case .next:
      guard canPlayRemoteCommands() else {
        return
      }
      setAudioSessionActive(true)
      _ = callSync(method: "audioPlayNext", payload: "true")
    case .previous:
      guard canPlayRemoteCommands() else {
        return
      }
      setAudioSessionActive(true)
      _ = callSync(method: "audioPlayPrevious", payload: "null")
    case .stop:
      _ = callSync(method: "audioStop", payload: "null")
      setAudioSessionActive(false)
    }
  }

  private func configureCommandAvailability(_ payload: [String: Any]) {
    let commandCenter = MPRemoteCommandCenter.shared()
    let canPlay = boolValue(payload["can_play"])
    let isPlaying = boolValue(payload["is_playing"])
    setCanPlayRemoteCommands(canPlay)
    commandCenter.nextTrackCommand.isEnabled = canPlay && boolValue(payload["can_next"])
    commandCenter.previousTrackCommand.isEnabled = canPlay && boolValue(payload["can_previous"])
    commandCenter.changePlaybackPositionCommand.isEnabled = boolValue(payload["can_seek"])
    commandCenter.playCommand.isEnabled = canPlay
    commandCenter.pauseCommand.isEnabled = canPlay || isPlaying
    commandCenter.togglePlayPauseCommand.isEnabled = canPlay || isPlaying
    commandCenter.stopCommand.isEnabled = true
  }

  private func artworkValue(_ uri: String) -> MPMediaItemArtwork? {
    let cacheKey = uri as NSString
    if let artwork = artworkCache.object(forKey: cacheKey) {
      return artwork
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

    let preparedImage = image.preparingForDisplay() ?? image
    let artwork = MPMediaItemArtwork(boundsSize: preparedImage.size) { _ in preparedImage }
    artworkCache.setObject(artwork, forKey: cacheKey)
    return artwork
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
