import Darwin
import Foundation
import Network
import UIKit

private struct ResourceDiagnosticsSession: Codable {
  let id: String
  let startedAt: String
  var stoppedAt: String?
  var stoppedReason: String?
  var sampleCount: Int
  var active: Bool

  enum CodingKeys: String, CodingKey {
    case id
    case startedAt = "started_at"
    case stoppedAt = "stopped_at"
    case stoppedReason = "stopped_reason"
    case sampleCount = "sample_count"
    case active
  }
}

final class ResourceDiagnosticsCollector {
  private let queue = DispatchQueue(
    label: "dev.xikxp1.stereodrome.mobile.resource-diagnostics",
    qos: .utility
  )
  private let playbackSnapshot: () -> [String: Any]
  private let directory: URL
  private let metadataFile: URL
  private let samplesFile: URL
  private let isoFormatter = ISO8601DateFormatter()
  private var session: ResourceDiagnosticsSession?
  private var timer: DispatchSourceTimer?
  private var networkMonitor: NWPathMonitor?
  private var networkStatus: [String: Any] = [
    "connected": false,
    "expensive": false,
    "constrained": false,
    "transports": [],
  ]
  private var previousCPUTimeMs: Double?
  private var previousSampleUptimeMs: Double?
  private var cachedLifecycle = "unknown"
  private var cachedBattery: [String: Any] = [
    "level_percent": NSNull(),
    "state": "unknown",
    "low_power_mode": false,
  ]
  private var deviceStateObservers: [NSObjectProtocol] = []
  private var batteryMonitoringWasEnabled = false
  private var managesBatteryMonitoring = false

  init(playbackSnapshot: @escaping () -> [String: Any]) {
    self.playbackSnapshot = playbackSnapshot
    let base = FileManager.default.urls(
      for: .applicationSupportDirectory,
      in: .userDomainMask
    ).first!
    directory = base.appendingPathComponent(
      "stereodrome-resource-diagnostics",
      isDirectory: true
    )
    metadataFile = directory.appendingPathComponent("session.json")
    samplesFile = directory.appendingPathComponent("samples.ndjson")
    session = Self.readSession(from: metadataFile)
    queue.sync {
      if session?.active == true {
        startSamplingLocked()
      }
    }
  }

  deinit {
    timer?.cancel()
    networkMonitor?.cancel()
  }

  func start() throws -> String {
    let initialDeviceState = deviceStateSnapshot()
    return try queue.sync {
      stopSamplingLocked()
      try prepareDirectoryLocked()
      try? FileManager.default.removeItem(at: samplesFile)
      session = ResourceDiagnosticsSession(
        id: UUID().uuidString.lowercased(),
        startedAt: now(),
        stoppedAt: nil,
        stoppedReason: nil,
        sampleCount: 0,
        active: true
      )
      previousCPUTimeMs = nil
      previousSampleUptimeMs = nil
      cachedLifecycle = initialDeviceState.lifecycle
      cachedBattery = initialDeviceState.battery
      try writeSessionLocked()
      try appendSampleLocked()
      startSamplingLocked()
      return try statusJSONLocked()
    }
  }

  func stop() throws -> String {
    try queue.sync {
      if session?.active == true {
        try appendSampleLocked()
        if session?.active == true {
          session?.active = false
          session?.stoppedAt = now()
          session?.stoppedReason = "manual"
          try writeSessionLocked()
        }
      }
      stopSamplingLocked()
      return try statusJSONLocked()
    }
  }

  func status() throws -> String {
    try queue.sync {
      try statusJSONLocked()
    }
  }

  func clear() -> Bool {
    queue.sync {
      stopSamplingLocked()
      session = nil
      previousCPUTimeMs = nil
      previousSampleUptimeMs = nil
      try? FileManager.default.removeItem(at: directory)
      return true
    }
  }

  func export(to destinationPath: String) throws -> Bool {
    try queue.sync {
      guard let session else {
        throw ResourceDiagnosticsError.noSession
      }
      let samples = try readSamplesLocked()
      let report: [String: Any] = [
        "schema_version": 1,
        "kind": "stereodrome-mobile-resource-diagnostics",
        "exported_at": now(),
        "session": sessionDictionary(session),
        "app": appDictionary(),
        "metric_definitions": metricDefinitionsDictionary(),
        "privacy": [
          "contains_account_credentials": false,
          "contains_server_urls": false,
          "contains_media_metadata": false,
          "excluded_fields": [
            "passwords", "tokens", "server URLs", "song titles", "artists", "albums",
          ],
        ],
        "samples": samples,
      ]
      let data = try JSONSerialization.data(
        withJSONObject: report,
        options: [.prettyPrinted, .sortedKeys]
      )
      let destination = URL(fileURLWithPath: destinationPath)
      try FileManager.default.createDirectory(
        at: destination.deletingLastPathComponent(),
        withIntermediateDirectories: true
      )
      try data.write(to: destination, options: .atomic)
      return true
    }
  }

  func close() {
    queue.sync {
      stopSamplingLocked()
    }
  }

  private func startSamplingLocked() {
    guard timer == nil, session?.active == true else {
      return
    }
    startNetworkMonitorLocked()
    setDeviceStateMonitoringEnabled(true)
    let timer = DispatchSource.makeTimerSource(queue: queue)
    timer.schedule(
      deadline: .now() + .seconds(Self.sampleIntervalSeconds),
      repeating: .seconds(Self.sampleIntervalSeconds),
      leeway: .seconds(1)
    )
    timer.setEventHandler { [weak self] in
      self?.collectScheduledSampleLocked()
    }
    self.timer = timer
    timer.resume()
  }

  private func stopSamplingLocked() {
    timer?.cancel()
    timer = nil
    networkMonitor?.cancel()
    networkMonitor = nil
    setDeviceStateMonitoringEnabled(false)
  }

  private func collectScheduledSampleLocked() {
    guard session?.active == true else {
      stopSamplingLocked()
      return
    }
    do {
      try appendSampleLocked()
    } catch {
      // Keep the diagnostics session non-fatal. A later sample can recover from
      // a transient file-system failure, while status/export still expose what
      // was persisted successfully.
    }
  }

  private func appendSampleLocked() throws {
    guard var current = session else {
      return
    }
    if current.sampleCount >= Self.maxSamples {
      try finishAtLimitLocked(&current)
      return
    }
    try prepareDirectoryLocked()
    let sample = sampleDictionary(session: current)
    var data = try JSONSerialization.data(withJSONObject: sample, options: [.sortedKeys])
    data.append(0x0A)
    if !FileManager.default.fileExists(atPath: samplesFile.path) {
      try data.write(to: samplesFile, options: .atomic)
    } else {
      let handle = try FileHandle(forWritingTo: samplesFile)
      defer { try? handle.close() }
      try handle.seekToEnd()
      try handle.write(contentsOf: data)
    }
    current.sampleCount += 1
    if current.sampleCount >= Self.maxSamples {
      try finishAtLimitLocked(&current)
    } else {
      session = current
      try writeSessionLocked()
    }
  }

  private func finishAtLimitLocked(_ current: inout ResourceDiagnosticsSession) throws {
    current.active = false
    current.stoppedAt = now()
    current.stoppedReason = "sample_limit"
    session = current
    try writeSessionLocked()
    stopSamplingLocked()
  }

  private func sampleDictionary(session: ResourceDiagnosticsSession) -> [String: Any] {
    let uptimeMs = ProcessInfo.processInfo.systemUptime * 1_000
    let cpuTimeMs = processCPUTimeMs()
    let cpuPercent: Any
    if let previousCPUTimeMs, let previousSampleUptimeMs, uptimeMs > previousSampleUptimeMs {
      cpuPercent = max(0, cpuTimeMs - previousCPUTimeMs) * 100 / (uptimeMs - previousSampleUptimeMs)
    } else {
      cpuPercent = NSNull()
    }
    previousCPUTimeMs = cpuTimeMs
    previousSampleUptimeMs = uptimeMs
    let memory = processMemoryDictionary()
    let storage = storageDictionary()
    let networkCounters = networkByteCounters()
    var network = networkStatus
    network["received_bytes"] = networkCounters.received
    network["transmitted_bytes"] = networkCounters.transmitted
    network["scope"] = "device_interfaces"

    return [
      "timestamp": now(),
      "elapsed_since_start_ms": elapsedSinceStartMs(session),
      "lifecycle": cachedLifecycle,
      "playback": playbackSnapshot(),
      "process": [
        "cpu_time_ms": cpuTimeMs,
        "cpu_percent_since_previous": cpuPercent,
        "resident_memory_bytes": memory.resident,
        "physical_footprint_bytes": memory.footprint,
        "virtual_memory_bytes": memory.virtual,
        "thread_count": processThreadCount(),
      ],
      "battery": cachedBattery,
      "thermal_state": thermalState(),
      "network": network,
      "storage": storage,
    ]
  }

  private func startNetworkMonitorLocked() {
    guard networkMonitor == nil else {
      return
    }
    let monitor = NWPathMonitor()
    monitor.pathUpdateHandler = { [weak self] path in
      var transports: [String] = []
      if path.usesInterfaceType(.wifi) { transports.append("wifi") }
      if path.usesInterfaceType(.cellular) { transports.append("cellular") }
      if path.usesInterfaceType(.wiredEthernet) { transports.append("ethernet") }
      if path.usesInterfaceType(.other) { transports.append("other") }
      self?.networkStatus = [
        "connected": path.status == .satisfied,
        "expensive": path.isExpensive,
        "constrained": path.isConstrained,
        "transports": transports,
      ]
    }
    networkMonitor = monitor
    monitor.start(queue: queue)
  }

  private func deviceStateSnapshot() -> (
    lifecycle: String,
    battery: [String: Any]
  ) {
    if Thread.isMainThread {
      return deviceStateSnapshotOnMain()
    }
    return DispatchQueue.main.sync {
      deviceStateSnapshotOnMain()
    }
  }

  private func deviceStateSnapshotOnMain() -> (
    lifecycle: String,
    battery: [String: Any]
  ) {
    let device = UIDevice.current
    let level: Any = device.batteryLevel >= 0 ? Double(device.batteryLevel) * 100 : NSNull()
    let state: String
    switch device.batteryState {
    case .charging: state = "charging"
    case .full: state = "full"
    case .unplugged: state = "discharging"
    default: state = "unknown"
    }
    let lifecycle: String
    switch UIApplication.shared.applicationState {
    case .active: lifecycle = "foreground"
    case .inactive: lifecycle = "inactive"
    case .background: lifecycle = "background"
    @unknown default: lifecycle = "unknown"
    }
    return (
      lifecycle,
      [
        "level_percent": level,
        "state": state,
        "low_power_mode": ProcessInfo.processInfo.isLowPowerModeEnabled,
      ]
    )
  }

  private func setDeviceStateMonitoringEnabled(_ enabled: Bool) {
    DispatchQueue.main.async { [weak self] in
      guard let self else {
        return
      }
      if enabled, !self.managesBatteryMonitoring {
        batteryMonitoringWasEnabled = UIDevice.current.isBatteryMonitoringEnabled
        UIDevice.current.isBatteryMonitoringEnabled = true
        managesBatteryMonitoring = true
        let center = NotificationCenter.default
        let notifications = [
          UIApplication.didBecomeActiveNotification,
          UIApplication.willResignActiveNotification,
          UIApplication.didEnterBackgroundNotification,
          UIDevice.batteryLevelDidChangeNotification,
          UIDevice.batteryStateDidChangeNotification,
          Notification.Name.NSProcessInfoPowerStateDidChange,
        ]
        deviceStateObservers = notifications.map { notification in
          center.addObserver(
            forName: notification,
            object: nil,
            queue: .main
          ) { [weak self] _ in
            self?.refreshCachedDeviceStateFromMain()
          }
        }
        refreshCachedDeviceStateFromMain()
      } else if !enabled, self.managesBatteryMonitoring {
        let center = NotificationCenter.default
        deviceStateObservers.forEach(center.removeObserver)
        deviceStateObservers.removeAll()
        UIDevice.current.isBatteryMonitoringEnabled = batteryMonitoringWasEnabled
        managesBatteryMonitoring = false
      }
    }
  }

  private func refreshCachedDeviceStateFromMain() {
    let state = deviceStateSnapshotOnMain()
    queue.async { [weak self] in
      self?.cachedLifecycle = state.lifecycle
      self?.cachedBattery = state.battery
    }
  }

  private func thermalState() -> String {
    switch ProcessInfo.processInfo.thermalState {
    case .nominal: return "nominal"
    case .fair: return "fair"
    case .serious: return "serious"
    case .critical: return "critical"
    @unknown default: return "unknown"
    }
  }

  private func processCPUTimeMs() -> Double {
    var usage = rusage()
    guard getrusage(RUSAGE_SELF, &usage) == 0 else {
      return 0
    }
    let user = Double(usage.ru_utime.tv_sec) * 1_000 + Double(usage.ru_utime.tv_usec) / 1_000
    let system = Double(usage.ru_stime.tv_sec) * 1_000 + Double(usage.ru_stime.tv_usec) / 1_000
    return user + system
  }

  private func metricDefinitionsDictionary() -> [String: String] {
    [
      "sample_interval": "10 seconds while the app process is running",
      "cpu_percent": "Process CPU used between samples as a percentage of one logical processor",
      "memory": "Current Stereodrome process memory",
      "network": "Cumulative device-interface byte counters since boot",
      "storage": "Current device volume capacity",
      "battery": "Current device battery and power state",
    ]
  }

  private func processMemoryDictionary() -> (resident: UInt64, footprint: UInt64, virtual: UInt64) {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(
      MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size
    )
    let result = withUnsafeMutablePointer(to: &info) { pointer in
      pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
        task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
      }
    }
    guard result == KERN_SUCCESS else {
      return (0, 0, 0)
    }
    return (UInt64(info.resident_size), UInt64(info.phys_footprint), UInt64(info.virtual_size))
  }

  private func processThreadCount() -> Int {
    var threads: thread_act_array_t?
    var count: mach_msg_type_number_t = 0
    guard task_threads(mach_task_self_, &threads, &count) == KERN_SUCCESS, let threads else {
      return 0
    }
    // task_threads hands us a send right per thread; releasing only the array
    // would leak one Mach port per thread on every sample.
    for index in 0..<Int(count) {
      mach_port_deallocate(mach_task_self_, threads[index])
    }
    let size = vm_size_t(Int(count) * MemoryLayout<thread_t>.stride)
    vm_deallocate(mach_task_self_, vm_address_t(bitPattern: threads), size)
    return Int(count)
  }

  private func networkByteCounters() -> (received: UInt64, transmitted: UInt64) {
    var interfaces: UnsafeMutablePointer<ifaddrs>?
    guard getifaddrs(&interfaces) == 0, let first = interfaces else {
      return (0, 0)
    }
    defer { freeifaddrs(interfaces) }
    var received: UInt64 = 0
    var transmitted: UInt64 = 0
    var current: UnsafeMutablePointer<ifaddrs>? = first
    while let interface = current {
      let value = interface.pointee
      if
        value.ifa_flags & UInt32(IFF_UP) != 0,
        value.ifa_addr?.pointee.sa_family == UInt8(AF_LINK),
        let rawData = value.ifa_data
      {
        let data = rawData.assumingMemoryBound(to: if_data.self).pointee
        received += UInt64(data.ifi_ibytes)
        transmitted += UInt64(data.ifi_obytes)
      }
      current = value.ifa_next
    }
    return (received, transmitted)
  }

  private func storageDictionary() -> [String: Any] {
    let values = try? directory.resourceValues(forKeys: [
      .volumeAvailableCapacityForImportantUsageKey,
      .volumeTotalCapacityKey,
    ])
    return [
      "available_bytes": values?.volumeAvailableCapacityForImportantUsage ?? 0,
      "total_bytes": values?.volumeTotalCapacity ?? 0,
    ]
  }

  private func appDictionary() -> [String: Any] {
    let info = Bundle.main.infoDictionary ?? [:]
    return [
      "platform": "ios",
      "application_id": Bundle.main.bundleIdentifier ?? "unknown",
      "version": info["CFBundleShortVersionString"] as? String ?? "unknown",
      "build": info["CFBundleVersion"] as? String ?? "unknown",
      "os_version": UIDevice.current.systemVersion,
      "device_model": machineIdentifier(),
      "architecture": architectureIdentifier,
      "processor_count": ProcessInfo.processInfo.activeProcessorCount,
      "physical_memory_bytes": ProcessInfo.processInfo.physicalMemory,
      "low_power_mode_supported": true,
      "simulator": isSimulator,
    ]
  }

  private func machineIdentifier() -> String {
    var systemInfo = utsname()
    uname(&systemInfo)
    return withUnsafePointer(to: &systemInfo.machine) {
      $0.withMemoryRebound(to: CChar.self, capacity: 1) {
        String(cString: $0)
      }
    }
  }

  private var isSimulator: Bool {
    #if targetEnvironment(simulator)
      return true
    #else
      return false
    #endif
  }

  private var architectureIdentifier: String {
    #if arch(arm64)
      return "arm64"
    #elseif arch(x86_64)
      return "x86_64"
    #else
      return "unknown"
    #endif
  }

  private func statusJSONLocked() throws -> String {
    let value: [String: Any] = [
      "active": session?.active == true,
      "started_at": session?.startedAt ?? NSNull(),
      "stopped_at": session?.stoppedAt ?? NSNull(),
      "stopped_reason": session?.stoppedReason ?? NSNull(),
      "sample_count": session?.sampleCount ?? 0,
      "sampling_interval_seconds": Self.sampleIntervalSeconds,
      "max_samples": Self.maxSamples,
    ]
    let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    return String(decoding: data, as: UTF8.self)
  }

  private func sessionDictionary(_ value: ResourceDiagnosticsSession) -> [String: Any] {
    [
      "id": value.id,
      "started_at": value.startedAt,
      "stopped_at": value.stoppedAt ?? NSNull(),
      "stopped_reason": value.stoppedReason ?? NSNull(),
      "active": value.active,
      "sample_count": value.sampleCount,
      "sampling_interval_seconds": Self.sampleIntervalSeconds,
      "max_samples": Self.maxSamples,
      "truncated": value.stoppedReason == "sample_limit",
    ]
  }

  private func readSamplesLocked() throws -> [[String: Any]] {
    guard FileManager.default.fileExists(atPath: samplesFile.path) else {
      return []
    }
    let contents = try String(contentsOf: samplesFile, encoding: .utf8)
    return try contents.split(separator: "\n").map { line in
      let data = Data(line.utf8)
      guard let sample = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
        throw ResourceDiagnosticsError.invalidSamples
      }
      return sample
    }
  }

  private func writeSessionLocked() throws {
    guard let session else {
      return
    }
    try prepareDirectoryLocked()
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    try encoder.encode(session).write(to: metadataFile, options: .atomic)
  }

  private func prepareDirectoryLocked() throws {
    try FileManager.default.createDirectory(
      at: directory,
      withIntermediateDirectories: true
    )
    var values = URLResourceValues()
    values.isExcludedFromBackup = true
    var mutableDirectory = directory
    try? mutableDirectory.setResourceValues(values)
  }

  private func elapsedSinceStartMs(_ session: ResourceDiagnosticsSession) -> Int64 {
    guard let startedAt = isoFormatter.date(from: session.startedAt) else {
      return 0
    }
    return max(0, Int64(Date().timeIntervalSince(startedAt) * 1_000))
  }

  private func now() -> String {
    isoFormatter.string(from: Date())
  }

  private static func readSession(from file: URL) -> ResourceDiagnosticsSession? {
    guard let data = try? Data(contentsOf: file) else {
      return nil
    }
    return try? JSONDecoder().decode(ResourceDiagnosticsSession.self, from: data)
  }

  private static let sampleIntervalSeconds = 10
  private static let maxSamples = 8_640
}

private enum ResourceDiagnosticsError: LocalizedError {
  case noSession
  case invalidSamples

  var errorDescription: String? {
    switch self {
    case .noSession:
      return "No diagnostics session is available"
    case .invalidSamples:
      return "The persisted diagnostics samples are invalid"
    }
  }
}
