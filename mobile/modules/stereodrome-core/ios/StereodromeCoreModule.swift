import AVFoundation
import ExpoModulesCore
import MediaPlayer
import UIKit

@_silgen_name("stereodrome_runtime_new")
private func stereodromeRuntimeNew(_ dataDir: UnsafePointer<CChar>) -> OpaquePointer?

@_silgen_name("stereodrome_runtime_destroy")
private func stereodromeRuntimeDestroy(_ core: OpaquePointer?)

@_silgen_name("stereodrome_core_call")
private func stereodromeCoreCall(
  _ core: OpaquePointer?,
  _ method: UnsafePointer<CChar>,
  _ payload: UnsafePointer<CChar>
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("stereodrome_core_free_string")
private func stereodromeCoreFreeString(_ value: UnsafeMutablePointer<CChar>?)

@_silgen_name("stereodrome_runtime_dispatch")
private func stereodromeRuntimeDispatch(
  _ core: OpaquePointer?,
  _ commandJson: UnsafePointer<CChar>
) -> UnsafeMutablePointer<CChar>?

private typealias StereodromeRustLogCallback = @convention(c) (UnsafePointer<CChar>?) -> Void
private typealias StereodromeEventCallback = @convention(c) (
  UnsafePointer<CChar>?, UnsafeMutableRawPointer?
) -> Void
private let playbackPositionDeduplicationToleranceSeconds = 0.25

fileprivate struct PlaybackProjection {
  let isStopped: Bool
  let songId: String
  let title: String
  let artist: String
  let album: String
  let durationSeconds: Double
  let positionSeconds: Double
  let isPlaying: Bool
  let outputState: String
  let artworkUri: String?
  let queueIndex: Int?
  let queueCount: Int
  let canNext: Bool
  let canPlay: Bool
  let canPrevious: Bool
  let canSeek: Bool

  private init(
    isStopped: Bool,
    songId: String,
    title: String,
    artist: String,
    album: String,
    durationSeconds: Double,
    positionSeconds: Double,
    isPlaying: Bool,
    outputState: String,
    artworkUri: String?,
    queueIndex: Int?,
    queueCount: Int,
    canNext: Bool,
    canPlay: Bool,
    canPrevious: Bool,
    canSeek: Bool
  ) {
    self.isStopped = isStopped
    self.songId = songId
    self.title = title
    self.artist = artist
    self.album = album
    self.durationSeconds = durationSeconds
    self.positionSeconds = positionSeconds
    self.isPlaying = isPlaying
    self.outputState = outputState
    self.artworkUri = artworkUri
    self.queueIndex = queueIndex
    self.queueCount = queueCount
    self.canNext = canNext
    self.canPlay = canPlay
    self.canPrevious = canPrevious
    self.canSeek = canSeek
  }

  private static func stringValue(_ value: Any?) -> String? {
    if let value = value as? String, !value.isEmpty {
      return value
    }
    return nil
  }

  private static func doubleValue(_ value: Any?) -> Double {
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

  private static func intValue(_ value: Any?) -> Int? {
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

  private static func boolValue(_ value: Any?) -> Bool {
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

  init?(eventJson: String) {
    guard
      let data = eventJson.data(using: .utf8),
      let event = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
      Self.stringValue(event["type"]) == "platform-projection",
      let payload = event["payload"] as? [String: Any],
      Self.intValue(payload["protocol_version"]) == 1,
      let snapshot = payload["projection"] as? [String: Any]
    else {
      return nil
    }

    guard
      Self.stringValue(snapshot["state"]) != "stopped",
      let song = snapshot["song"] as? [String: Any]
    else {
      self = PlaybackProjection.stopped
      return
    }

    let duration = Self.doubleValue(snapshot["duration_seconds"]) > 0
      ? Self.doubleValue(snapshot["duration_seconds"])
      : Self.doubleValue(song["duration_seconds"])
    self = PlaybackProjection(
      isStopped: false,
      songId: Self.stringValue(song["id"]) ?? "",
      title: Self.stringValue(song["title"]) ?? "Unknown Title",
      artist: Self.stringValue(song["artist"]) ?? "Unknown Artist",
      album: Self.stringValue(song["album"]) ?? "Unknown Album",
      durationSeconds: duration,
      positionSeconds: Self.doubleValue(snapshot["position_seconds"]),
      isPlaying: Self.boolValue(snapshot["is_playing"]),
      outputState: Self.stringValue(snapshot["output_state"]) ?? "closed",
      artworkUri: Self.stringValue(song["artwork_uri"]),
      queueIndex: Self.intValue(snapshot["queue_index"]),
      queueCount: Self.intValue(snapshot["queue_length"]) ?? 0,
      canNext: Self.boolValue(snapshot["can_next"]),
      canPlay: Self.boolValue(snapshot["can_play"]),
      canPrevious: Self.boolValue(snapshot["can_previous"]),
      canSeek: Self.boolValue(snapshot["can_seek"])
    )
  }

  func hasSameProjection(as other: PlaybackProjection) -> Bool {
    let positionMatches = positionSeconds == other.positionSeconds
      || (isPlaying && other.isPlaying
        && abs(positionSeconds - other.positionSeconds)
          < playbackPositionDeduplicationToleranceSeconds)
    return isStopped == other.isStopped
      && songId == other.songId
      && title == other.title
      && artist == other.artist
      && album == other.album
      && durationSeconds == other.durationSeconds
      && positionMatches
      && isPlaying == other.isPlaying
      && outputState == other.outputState
      && artworkUri == other.artworkUri
      && queueIndex == other.queueIndex
      && queueCount == other.queueCount
      && canNext == other.canNext
      && canPlay == other.canPlay
      && canPrevious == other.canPrevious
      && canSeek == other.canSeek
  }

  private static let stopped = PlaybackProjection(
    isStopped: true,
    songId: "",
    title: "",
    artist: "",
    album: "",
    durationSeconds: 0,
    positionSeconds: 0,
    isPlaying: false,
    outputState: "closed",
    artworkUri: nil,
    queueIndex: nil,
    queueCount: 0,
    canNext: false,
    canPlay: false,
    canPrevious: false,
    canSeek: false
  )
}

@_silgen_name("stereodrome_core_set_log_callback")
private func stereodromeCoreSetLogCallback(_ callback: StereodromeRustLogCallback?)

@_silgen_name("stereodrome_runtime_set_event_callback")
private func stereodromeRuntimeSetEventCallback(
  _ core: OpaquePointer?,
  _ callback: StereodromeEventCallback?,
  _ context: UnsafeMutableRawPointer?
)

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

private final class StereodromeEventCallbackContext {
  weak var module: StereodromeCoreModule?

  init(module: StereodromeCoreModule) {
    self.module = module
  }
}

private func stereodromeEventCallback(
  _ event: UnsafePointer<CChar>?,
  _ context: UnsafeMutableRawPointer?
) {
  guard let event, let context else {
    return
  }
  let callbackContext = Unmanaged<StereodromeEventCallbackContext>
    .fromOpaque(context)
    .takeUnretainedValue()
  callbackContext.module?.handleCoreEvent(String(cString: event))
}

public class StereodromeCoreModule: Module {
  private struct AudioSessionLease {
    let acquiredNow: Bool
  }

  private enum AudioSessionAcquisition {
    case acquired(AudioSessionLease)
    case failed(String)
  }

  private var core: OpaquePointer?
  private let coreQueue = DispatchQueue(label: "dev.xikxp1.stereodrome.mobile.core")
  private let remoteCommandQueue = DispatchQueue(
    label: "dev.xikxp1.stereodrome.mobile.remote-commands")
  private let artworkCache = NSCache<NSString, MPMediaItemArtwork>()
  private let artworkQueue = DispatchQueue(
    label: "dev.xikxp1.stereodrome.mobile.artwork", qos: .utility)
  private let playbackProjectionLock = NSLock()
  private var playbackProjection: PlaybackProjection?
  private var currentArtworkUri: String?
  private let remoteCommandStateLock = NSLock()
  private var remoteCommandTargets: [Any] = []
  private var audioSessionObservers: [NSObjectProtocol] = []
  private var canPlayRemoteCommandsValue = false
  private let audioSessionStateLock = NSLock()
  private var ownsAudioSession = false
  private var audioSessionGeneration: UInt64 = 0
  private var eventCallbackContext: UnsafeMutableRawPointer?
  private var nextNativeCommandId = Int64.max

  deinit {
    clearAudioSessionObservers()
    if clearActiveStereodromeCoreModule(self) {
      performOnMainSync {
        self.clearNowPlayingInfo()
      }
    } else {
      clearRemoteCommandHandlers()
    }
    releaseAudioSession()
    let coreToDestroy = core
    let callbackContextToRelease = eventCallbackContext
    core = nil
    eventCallbackContext = nil
    coreQueue.async {
      stereodromeRuntimeSetEventCallback(coreToDestroy, nil, nil)
      stereodromeRuntimeDestroy(coreToDestroy)
      if let callbackContextToRelease {
        Unmanaged<StereodromeEventCallbackContext>
          .fromOpaque(callbackContextToRelease)
          .release()
      }
    }
  }

  public func definition() -> ModuleDefinition {
    Name("StereodromeCore")

    AsyncFunction("initialize") { (_ dataDir: String) -> Bool in
      setActiveStereodromeCoreModule(self)
      stereodromeCoreSetLogCallback(stereodromeRustLogCallback)
      self.configureAudioSession()
      if self.core == nil {
        self.core = self.coreQueue.sync {
          dataDir.withCString { stereodromeRuntimeNew($0) }
        }
        if self.core != nil {
          let callbackContext = Unmanaged.passRetained(
            StereodromeEventCallbackContext(module: self)
          ).toOpaque()
          self.eventCallbackContext = callbackContext
          stereodromeRuntimeSetEventCallback(
            self.core,
            stereodromeEventCallback,
            callbackContext
          )
        }
      }
      self.configureAudioSessionObservers()
      return self.core != nil
    }

    AsyncFunction("call") { (_ method: String, _ payload: String) -> String in
      return self.callSync(method: method, payload: payload)
    }

    AsyncFunction("dispatch") { (_ commandJson: String) -> String in
      return self.coreQueue.sync {
        guard let core = self.core else {
          return #"{"protocol_version":1,"command_id":0,"accepted_revision":0,"operation_id":null,"status":"failed","error":{"code":"runtime_unavailable","message":"Stereodrome Rust core is not initialized","retryable":true}}"#
        }
        return commandJson.withCString { commandPointer in
          guard let resultPointer = stereodromeRuntimeDispatch(core, commandPointer) else {
            return #"{"protocol_version":1,"command_id":0,"accepted_revision":0,"operation_id":null,"status":"failed","error":{"code":"internal","message":"Rust returned null","retryable":false}}"#
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

    Events("core-event")
  }

  fileprivate func emitRustLog(_ message: String) {
    appContext?.jsLogger.info(message)
  }

  fileprivate func reservePlaybackProjection(_ projection: PlaybackProjection) -> Bool {
    playbackProjectionLock.lock()
    defer { playbackProjectionLock.unlock() }
    if playbackProjection?.hasSameProjection(as: projection) == true {
      return false
    }
    playbackProjection = projection
    return true
  }

  @discardableResult
  fileprivate func invalidatePlaybackProjection() -> PlaybackProjection? {
    playbackProjectionLock.lock()
    let previous = playbackProjection
    playbackProjection = nil
    playbackProjectionLock.unlock()
    return previous
  }

  fileprivate func reservePlaybackProjectionIfMissing(_ projection: PlaybackProjection) -> Bool {
    playbackProjectionLock.lock()
    defer { playbackProjectionLock.unlock() }
    guard playbackProjection == nil else {
      return false
    }
    playbackProjection = projection
    return true
  }

  fileprivate func replayPlaybackProjectionIfCurrent(_ projection: PlaybackProjection) {
    playbackProjectionLock.lock()
    defer { playbackProjectionLock.unlock() }
    guard playbackProjection?.hasSameProjection(as: projection) == true else {
      return
    }
    performOnMainSync {
      guard getActiveStereodromeCoreModule() === self else {
        return
      }
      _ = applyPlaybackProjection(projection)
    }
  }

  fileprivate func artworkUriNeedingLoad(for projection: PlaybackProjection) -> String? {
    guard
      !projection.isStopped,
      let artworkUri = projection.artworkUri,
      artworkCache.object(forKey: artworkUri as NSString) == nil
    else {
      return nil
    }
    return artworkUri
  }

  private func callSync(method: String, payload: String) -> String {
    return coreQueue.sync {
      callCore(method: method, payload: payload)
    }
  }

  private func callCore(method: String, payload: String) -> String {
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
    } catch {
      // Rust returns playback errors through the FFI call path; session setup
      // failure should not prevent core initialization in development builds.
    }
  }

  private func acquireAudioSession() -> AudioSessionAcquisition {
    audioSessionStateLock.lock()
    defer { audioSessionStateLock.unlock() }
    if ownsAudioSession {
      return .acquired(AudioSessionLease(acquiredNow: false))
    }

    let session = AVAudioSession.sharedInstance()
    do {
      try session.setCategory(.playback, mode: .default)
      try session.setActive(true)
      ownsAudioSession = true
      audioSessionGeneration &+= 1
      return .acquired(AudioSessionLease(acquiredNow: true))
    } catch {
      return .failed(error.localizedDescription)
    }
  }

  private func releaseAudioSession() {
    audioSessionStateLock.lock()
    defer { audioSessionStateLock.unlock() }
    guard ownsAudioSession else {
      return
    }
    do {
      try AVAudioSession.sharedInstance().setActive(
        false,
        options: [.notifyOthersOnDeactivation]
      )
      ownsAudioSession = false
      audioSessionGeneration &+= 1
    } catch {
      appContext?.jsLogger.warn("Failed to deactivate audio session: \(error.localizedDescription)")
    }
  }

  private func markAudioSessionInactive() -> UInt64 {
    audioSessionStateLock.lock()
    ownsAudioSession = false
    audioSessionGeneration &+= 1
    let generation = audioSessionGeneration
    audioSessionStateLock.unlock()
    return generation
  }

  private func isCurrentAudioSessionGeneration(_ expected: UInt64) -> Bool {
    audioSessionStateLock.lock()
    defer { audioSessionStateLock.unlock() }
    return audioSessionGeneration == expected
  }

  private func rollbackAudioSession(_ lease: AudioSessionLease) {
    if lease.acquiredNow {
      releaseAudioSession()
    }
  }

  private func dispatchCommand(
    _ command: [String: Any],
    activateAudioSession: Bool = false
  ) -> String {
    return coreQueue.sync {
      var lease: AudioSessionLease?
      if activateAudioSession {
        switch acquireAudioSession() {
        case .failed(let message):
          return runtimeError("Failed to activate audio session: \(message)")
        case .acquired(let acquiredLease):
          lease = acquiredLease
        }
      }
      let result = dispatchCommandCore(command)
      if let lease, !isSuccessfulRuntimeResult(result) {
        rollbackAudioSession(lease)
      }
      return result
    }
  }

  private func dispatchCommandCore(_ command: [String: Any]) -> String {
    guard let core else {
      return runtimeError("Stereodrome Rust core is not initialized")
    }
    let commandId = nextNativeCommandId
    nextNativeCommandId &-= 1
    guard
      let data = try? JSONSerialization.data(withJSONObject: [
        "protocol_version": 1,
        "command_id": commandId,
        "command": command,
      ]),
      let request = String(data: data, encoding: .utf8)
    else {
      return runtimeError("Failed to serialize native runtime command")
    }
    return request.withCString { commandPointer in
      guard let resultPointer = stereodromeRuntimeDispatch(core, commandPointer) else {
        return runtimeError("Rust returned null")
      }
      let result = String(cString: resultPointer)
      stereodromeCoreFreeString(resultPointer)
      return result
    }
  }

  private func dispatchPlatformEvent(
    _ event: [String: Any],
    activateAudioSession: Bool = false
  ) -> String {
    dispatchCommand(
      [
        "type": "report-platform-playback",
        "event": event,
      ],
      activateAudioSession: activateAudioSession
    )
  }

  private func isSuccessfulRuntimeResult(_ raw: String) -> Bool {
    guard
      let data = raw.data(using: .utf8),
      let envelope = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    else {
      return false
    }
    return stringValue(envelope["status"]) == "succeeded"
  }

  private func runtimeError(_ message: String) -> String {
    guard
      let data = try? JSONSerialization.data(withJSONObject: [
        "protocol_version": 1,
        "command_id": 0,
        "accepted_revision": 0,
        "operation_id": NSNull(),
        "status": "failed",
        "error": [
          "code": "runtime_unavailable",
          "message": message,
          "retryable": true,
        ],
      ]),
      let result = String(data: data, encoding: .utf8)
    else {
      return #"{"protocol_version":1,"command_id":0,"accepted_revision":0,"operation_id":null,"status":"failed","error":{"code":"internal","message":"Native adapter failed","retryable":false}}"#
    }
    return result
  }

  fileprivate func handleCoreEvent(_ event: String) {
    if let projection = PlaybackProjection(eventJson: event) {
      let artworkUri: String?
      if reservePlaybackProjection(projection) {
        artworkUri = performOnMainSync {
          guard getActiveStereodromeCoreModule() === self else {
            return nil
          }
          return applyPlaybackProjection(projection)
        }
      } else {
        artworkUri = artworkUriNeedingLoad(for: projection)
      }
      enqueueArtworkUpdate(artworkUri)
    }
    DispatchQueue.main.async { [weak self] in
      guard let self, getActiveStereodromeCoreModule() === self else {
        return
      }
      self.sendEvent("core-event", ["event": event])
    }
  }

  private func enqueueArtworkUpdate(_ artworkUri: String?) {
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

  fileprivate func applyPlaybackProjection(_ projection: PlaybackProjection) -> String? {
    if projection.isStopped {
      clearNowPlayingInfo()
      releaseAudioSession()
      return nil
    }
    if projection.isPlaying {
      switch acquireAudioSession() {
      case .acquired:
        break
      case .failed(let message):
        appContext?.jsLogger.warn("Failed to activate audio session: \(message)")
        remoteCommandQueue.async {
          _ = self.dispatchPlatformEvent([
            "type": "audio-focus-lost",
            "transient": false,
          ])
        }
      }
    }
    if projection.outputState == "unavailable" {
      releaseAudioSession()
    }

    var info: [String: Any] = [
      MPMediaItemPropertyTitle: projection.title,
      MPMediaItemPropertyArtist: projection.artist,
      MPMediaItemPropertyAlbumTitle: projection.album,
      MPMediaItemPropertyPlaybackDuration: projection.durationSeconds,
      MPNowPlayingInfoPropertyElapsedPlaybackTime: projection.positionSeconds,
      MPNowPlayingInfoPropertyPlaybackRate: projection.isPlaying ? 1.0 : 0.0,
      MPNowPlayingInfoPropertyPlaybackQueueCount: projection.queueCount,
    ]

    if let queueIndex = projection.queueIndex {
      info[MPNowPlayingInfoPropertyPlaybackQueueIndex] = queueIndex
    }

    let artworkUri = projection.artworkUri
    if let artworkUri,
      let artwork = artworkCache.object(forKey: artworkUri as NSString)
    {
      info[MPMediaItemPropertyArtwork] = artwork
    }

    currentArtworkUri = artworkUri
    MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    updateNowPlayingPlaybackState(isPlaying: projection.isPlaying)
    if remoteCommandTargets.isEmpty {
      configureRemoteCommandCenter()
    }
    configureCommandAvailability(projection)
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
      let interruptionGeneration = markAudioSessionInactive()
      // iOS has halted our audio output. Pause the Rust core so its state
      // matches reality; otherwise a later remote play command becomes a
      // no-op (resume on a sink that was never paused).
      remoteCommandQueue.async {
        guard self.isCurrentAudioSessionGeneration(interruptionGeneration) else {
          return
        }
        _ = self.dispatchPlatformEvent(["type": "interruption-began"])
      }
    case .ended:
      let optionsValue =
        notification.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
      let options = AVAudioSession.InterruptionOptions(rawValue: optionsValue)
      remoteCommandQueue.async {
        let shouldResume = options.contains(.shouldResume)
        _ = self.dispatchPlatformEvent(
          [
            "type": "interruption-ended",
            "should_resume": shouldResume,
          ]
        )
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
      _ = self.dispatchPlatformEvent(["type": "route-lost"])
    }
  }

  private func handleMediaServicesReset() {
    let projectionToRestore = invalidatePlaybackProjection()
    remoteCommandQueue.async {
      self.configureAudioSession()
      _ = self.markAudioSessionInactive()
      if let projectionToRestore,
        self.reservePlaybackProjectionIfMissing(projectionToRestore)
      {
        self.replayPlaybackProjectionIfCurrent(projectionToRestore)
      }
      _ = self.dispatchPlatformEvent(["type": "media-services-reset"])
    }
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
  /// calls can block while Rust prepares media. Acknowledge
  /// the command immediately and run it on a serial background queue.
  private func enqueueRemoteCommand(_ action: RemoteCommandAction) -> MPRemoteCommandHandlerStatus {
    remoteCommandQueue.async {
      self.performRemoteCommand(action)
    }
    return .success
  }

  private func enqueueRemoteSeek(_ positionSeconds: TimeInterval) -> MPRemoteCommandHandlerStatus {
    let positionSeconds = max(0.0, positionSeconds)
    remoteCommandQueue.async {
      _ = self.dispatchCommand([
        "type": "seek-to",
        "seconds": positionSeconds,
      ])
    }
    return .success
  }

  private func performRemoteCommand(_ action: RemoteCommandAction) {
    switch action {
    case .play:
      guard canPlayRemoteCommands() else {
        return
      }
      _ = dispatchCommand(["type": "resume-playback"], activateAudioSession: true)
    case .pause:
      _ = dispatchCommand(["type": "pause-playback"])
    case .toggle:
      if !canPlayRemoteCommands() {
        return
      }
      _ = dispatchCommand(["type": "toggle-playback"], activateAudioSession: true)
    case .next:
      guard canPlayRemoteCommands() else {
        return
      }
      _ = dispatchCommand(
        [
          "type": "navigate-playback",
          "navigation": ["type": "next", "force": true],
        ],
        activateAudioSession: true
      )
    case .previous:
      guard canPlayRemoteCommands() else {
        return
      }
      _ = dispatchCommand(
        [
          "type": "navigate-playback",
          "navigation": ["type": "previous"],
        ],
        activateAudioSession: true
      )
    case .stop:
      let result = dispatchCommand(["type": "stop-playback"])
      if isSuccessfulRuntimeResult(result) {
        releaseAudioSession()
      }
    }
  }

  private func configureCommandAvailability(_ projection: PlaybackProjection) {
    let commandCenter = MPRemoteCommandCenter.shared()
    let canPlay = projection.canPlay
    let isPlaying = projection.isPlaying
    setCanPlayRemoteCommands(canPlay)
    commandCenter.nextTrackCommand.isEnabled = canPlay && projection.canNext
    commandCenter.previousTrackCommand.isEnabled = canPlay && projection.canPrevious
    commandCenter.changePlaybackPositionCommand.isEnabled = projection.canSeek
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

  private func stringValue(_ value: Any?) -> String? {
    if let value = value as? String, !value.isEmpty {
      return value
    }
    return nil
  }

}
