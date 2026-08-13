package expo.modules.stereodromecore

import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ApplicationInfo
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.TrafficStats
import android.os.BatteryManager
import android.os.Build
import android.os.Debug
import android.os.PowerManager
import android.os.Process
import android.os.StatFs
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.time.Instant
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledExecutorService
import java.util.concurrent.TimeUnit

internal class ResourceDiagnosticsCollector(context: Context) {
  private data class Session(
    val id: String,
    val startedAt: String,
    var stoppedAt: String?,
    var stoppedReason: String?,
    var sampleCount: Int,
    var active: Boolean,
  )

  private val applicationContext = context.applicationContext
  private val lock = Any()
  private val directory = File(applicationContext.filesDir, "resource-diagnostics")
  private val metadataFile = File(directory, "session.json")
  private val samplesFile = File(directory, "samples.ndjson")
  private var session: Session? = readSession()
  private var scheduler: ScheduledExecutorService? = null
  private var previousCpuTimeMs: Long? = null
  private var previousSampleElapsedMs: Long? = null

  init {
    synchronized(lock) {
      if (session?.active == true) {
        startSchedulerLocked()
      }
    }
  }

  fun start(): String = synchronized(lock) {
    stopSchedulerLocked()
    directory.mkdirs()
    samplesFile.delete()
    val next = Session(
      id = UUID.randomUUID().toString(),
      startedAt = now(),
      stoppedAt = null,
      stoppedReason = null,
      sampleCount = 0,
      active = true,
    )
    session = next
    previousCpuTimeMs = null
    previousSampleElapsedMs = null
    writeSessionLocked(next)
    appendSampleLocked(next)
    startSchedulerLocked()
    statusJsonLocked().toString()
  }

  fun stop(): String = synchronized(lock) {
    session?.takeIf { it.active }?.let { current ->
      appendSampleLocked(current)
      if (current.active) {
        current.active = false
        current.stoppedAt = now()
        current.stoppedReason = "manual"
        writeSessionLocked(current)
      }
    }
    stopSchedulerLocked()
    statusJsonLocked().toString()
  }

  fun status(): String = synchronized(lock) {
    statusJsonLocked().toString()
  }

  fun clear(): Boolean = synchronized(lock) {
    stopSchedulerLocked()
    session = null
    previousCpuTimeMs = null
    previousSampleElapsedMs = null
    metadataFile.delete()
    samplesFile.delete()
    if (directory.exists() && directory.listFiles().isNullOrEmpty()) {
      directory.delete()
    }
    true
  }

  fun export(destinationPath: String): Boolean = synchronized(lock) {
    val current = session ?: throw IllegalStateException("No diagnostics session is available")
    val samples = JSONArray()
    if (samplesFile.exists()) {
      samplesFile.useLines { lines ->
        lines.filter { it.isNotBlank() }.forEach { samples.put(JSONObject(it)) }
      }
    }
    val report = JSONObject()
      .put("schema_version", 1)
      .put("kind", "stereodrome-mobile-resource-diagnostics")
      .put("exported_at", now())
      .put("session", sessionJson(current))
      .put("app", appJson())
      .put("metric_definitions", metricDefinitionsJson())
      .put(
        "privacy",
        JSONObject()
          .put("contains_account_credentials", false)
          .put("contains_server_urls", false)
          .put("contains_media_metadata", false)
          .put(
            "excluded_fields",
            JSONArray(listOf("passwords", "tokens", "server URLs", "song titles", "artists", "albums")),
          ),
      )
      .put("samples", samples)
    val destination = File(destinationPath)
    destination.parentFile?.mkdirs()
    val temporary = File(destination.parentFile, "${destination.name}.tmp")
    temporary.writeText(report.toString(2))
    if (destination.exists()) {
      destination.delete()
    }
    check(temporary.renameTo(destination)) { "Unable to finalize diagnostics report" }
    true
  }

  fun close() = synchronized(lock) {
    stopSchedulerLocked()
  }

  private fun startSchedulerLocked() {
    if (scheduler != null || session?.active != true) {
      return
    }
    scheduler = Executors.newSingleThreadScheduledExecutor { runnable ->
      Thread(runnable, "stereodrome-resource-diagnostics").apply { isDaemon = true }
    }.also { executor ->
      executor.scheduleAtFixedRate(
        { collectScheduledSample() },
        SAMPLE_INTERVAL_SECONDS,
        SAMPLE_INTERVAL_SECONDS,
        TimeUnit.SECONDS,
      )
    }
  }

  private fun stopSchedulerLocked() {
    scheduler?.shutdownNow()
    scheduler = null
  }

  private fun collectScheduledSample() = synchronized(lock) {
    val current = session ?: return@synchronized
    if (!current.active) {
      stopSchedulerLocked()
      return@synchronized
    }
    appendSampleLocked(current)
  }

  private fun appendSampleLocked(current: Session) {
    if (current.sampleCount >= MAX_SAMPLES) {
      finishAtLimitLocked(current)
      return
    }
    directory.mkdirs()
    samplesFile.appendText(sampleJson(current).toString() + "\n")
    current.sampleCount += 1
    if (current.sampleCount >= MAX_SAMPLES) {
      finishAtLimitLocked(current)
    } else {
      writeSessionLocked(current)
    }
  }

  private fun finishAtLimitLocked(current: Session) {
    current.active = false
    current.stoppedAt = now()
    current.stoppedReason = "sample_limit"
    writeSessionLocked(current)
    stopSchedulerLocked()
  }

  private fun sampleJson(current: Session): JSONObject {
    val elapsedRealtimeMs = android.os.SystemClock.elapsedRealtime()
    val cpuTimeMs = Process.getElapsedCpuTime()
    val cpuPercent = cpuPercent(cpuTimeMs, elapsedRealtimeMs)
    val memory = Debug.MemoryInfo().also(Debug::getMemoryInfo)
    val runtime = Runtime.getRuntime()
    val activityManager =
      applicationContext.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
    val processState = ActivityManager.RunningAppProcessInfo().also {
      ActivityManager.getMyMemoryState(it)
    }
    val storage = StatFs(applicationContext.filesDir.absolutePath)
    val playback = StereodromeMediaSessionState.diagnosticsSnapshot()

    return JSONObject()
      .put("timestamp", now())
      .put("elapsed_since_start_ms", elapsedSinceStartMs(current))
      .put("lifecycle", lifecycle(processState.importance))
      .put("playback", JSONObject(playback))
      .put(
        "process",
        JSONObject()
          .put("cpu_time_ms", cpuTimeMs)
          .putNullable("cpu_percent_since_previous", cpuPercent)
          .put("resident_memory_bytes", memory.totalPss.toLong() * 1024)
          .put("private_dirty_memory_bytes", memory.totalPrivateDirty.toLong() * 1024)
          .put("java_heap_used_bytes", runtime.totalMemory() - runtime.freeMemory())
          .put("java_heap_committed_bytes", runtime.totalMemory())
          .put("java_heap_limit_bytes", runtime.maxMemory())
          .put("memory_class_mb", activityManager.memoryClass)
          .put("large_memory_class_mb", activityManager.largeMemoryClass)
          .put("thread_count", Thread.getAllStackTraces().size),
      )
      .put("battery", batteryJson())
      .put("thermal_state", thermalState())
      .put("network", networkJson())
      .put(
        "storage",
        JSONObject()
          .put("available_bytes", storage.availableBytes)
          .put("total_bytes", storage.totalBytes),
      )
  }

  private fun batteryJson(): JSONObject {
    val intent = applicationContext.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
    val manager = applicationContext.getSystemService(Context.BATTERY_SERVICE) as BatteryManager
    val power = applicationContext.getSystemService(Context.POWER_SERVICE) as PowerManager
    val level = intent?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
    val scale = intent?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
    val status = intent?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1
    val state = when (status) {
      BatteryManager.BATTERY_STATUS_CHARGING -> "charging"
      BatteryManager.BATTERY_STATUS_FULL -> "full"
      BatteryManager.BATTERY_STATUS_DISCHARGING -> "discharging"
      BatteryManager.BATTERY_STATUS_NOT_CHARGING -> "not_charging"
      else -> "unknown"
    }
    return JSONObject()
      .putNullable("level_percent", if (level >= 0 && scale > 0) level * 100.0 / scale else null)
      .put("state", state)
      .put("low_power_mode", power.isPowerSaveMode)
      .putNullable(
        "temperature_celsius",
        intent?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, -1)
          ?.takeIf { it >= 0 }
          ?.div(10.0),
      )
      .putNullable(
        "current_microamps",
        manager.getLongProperty(BatteryManager.BATTERY_PROPERTY_CURRENT_NOW)
          .takeUnless { it == Long.MIN_VALUE },
      )
      .putNullable(
        "charge_counter_microamp_hours",
        manager.getLongProperty(BatteryManager.BATTERY_PROPERTY_CHARGE_COUNTER)
          .takeUnless { it == Long.MIN_VALUE },
      )
  }

  private fun thermalState(): String {
    val power = applicationContext.getSystemService(Context.POWER_SERVICE) as PowerManager
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
      return "unavailable"
    }
    return when (power.currentThermalStatus) {
      PowerManager.THERMAL_STATUS_NONE -> "nominal"
      PowerManager.THERMAL_STATUS_LIGHT -> "fair"
      PowerManager.THERMAL_STATUS_MODERATE -> "serious"
      PowerManager.THERMAL_STATUS_SEVERE -> "critical"
      PowerManager.THERMAL_STATUS_CRITICAL -> "critical"
      PowerManager.THERMAL_STATUS_EMERGENCY -> "emergency"
      PowerManager.THERMAL_STATUS_SHUTDOWN -> "shutdown"
      else -> "unknown"
    }
  }

  private fun networkJson(): JSONObject {
    val manager =
      applicationContext.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
    val capabilities = manager.getNetworkCapabilities(manager.activeNetwork)
    val transports = JSONArray()
    if (capabilities != null) {
      listOf(
        NetworkCapabilities.TRANSPORT_WIFI to "wifi",
        NetworkCapabilities.TRANSPORT_CELLULAR to "cellular",
        NetworkCapabilities.TRANSPORT_ETHERNET to "ethernet",
        NetworkCapabilities.TRANSPORT_VPN to "vpn",
        NetworkCapabilities.TRANSPORT_BLUETOOTH to "bluetooth",
      ).filter { capabilities.hasTransport(it.first) }
        .forEach { transports.put(it.second) }
    }
    val uid = applicationContext.applicationInfo.uid
    return JSONObject()
      .put("connected", capabilities != null)
      .put("transports", transports)
      .putNullable("device_received_bytes", counterOrNull(TrafficStats.getTotalRxBytes()))
      .putNullable("device_transmitted_bytes", counterOrNull(TrafficStats.getTotalTxBytes()))
      .putNullable("uid_received_bytes", counterOrNull(TrafficStats.getUidRxBytes(uid)))
      .putNullable("uid_transmitted_bytes", counterOrNull(TrafficStats.getUidTxBytes(uid)))
  }

  private fun metricDefinitionsJson(): JSONObject = JSONObject()
    .put("sample_interval", "10 seconds while the app process is running")
    .put("cpu_percent", "Process CPU used between samples as a percentage of one logical processor")
    .put("memory", "Current Stereodrome process memory")
    .put("network", "Cumulative device and Stereodrome application UID byte counters since boot")
    .put("storage", "Current device volume capacity")
    .put("battery", "Current device battery and power state; current sign is device-defined")

  private fun appJson(): JSONObject {
    val packageInfo = applicationContext.packageManager.getPackageInfo(applicationContext.packageName, 0)
    val architecture = Build.SUPPORTED_ABIS.firstOrNull() ?: "unknown"
    return JSONObject()
      .put("platform", "android")
      .put("application_id", applicationContext.packageName)
      .put("version", packageInfo.versionName ?: "unknown")
      .put("build", if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) packageInfo.longVersionCode else @Suppress("DEPRECATION") packageInfo.versionCode.toLong())
      .put("os_version", Build.VERSION.RELEASE)
      .put("os_api_level", Build.VERSION.SDK_INT)
      .put("device_manufacturer", Build.MANUFACTURER)
      .put("device_model", Build.MODEL)
      .put("architecture", architecture)
      .put("processor_count", Runtime.getRuntime().availableProcessors())
      .put("physical_memory_bytes", totalPhysicalMemoryBytes())
      .put(
        "debuggable",
        applicationContext.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0,
      )
      .put("emulator", isEmulator())
  }

  private fun statusJsonLocked(): JSONObject {
    val current = session
    return JSONObject()
      .put("active", current?.active == true)
      .putNullable("started_at", current?.startedAt)
      .putNullable("stopped_at", current?.stoppedAt)
      .putNullable("stopped_reason", current?.stoppedReason)
      .put("sample_count", current?.sampleCount ?: 0)
      .put("sampling_interval_seconds", SAMPLE_INTERVAL_SECONDS)
      .put("max_samples", MAX_SAMPLES)
  }

  private fun sessionJson(current: Session): JSONObject = JSONObject()
    .put("id", current.id)
    .put("started_at", current.startedAt)
    .putNullable("stopped_at", current.stoppedAt)
    .putNullable("stopped_reason", current.stoppedReason)
    .put("active", current.active)
    .put("sample_count", current.sampleCount)
    .put("sampling_interval_seconds", SAMPLE_INTERVAL_SECONDS)
    .put("max_samples", MAX_SAMPLES)
    .put("truncated", current.stoppedReason == "sample_limit")

  private fun writeSessionLocked(current: Session) {
    directory.mkdirs()
    val temporary = File(directory, "session.json.tmp")
    temporary.writeText(sessionJson(current).toString())
    if (metadataFile.exists()) {
      metadataFile.delete()
    }
    check(temporary.renameTo(metadataFile)) { "Unable to persist diagnostics session" }
  }

  private fun readSession(): Session? = try {
    if (!metadataFile.exists()) {
      null
    } else {
      val json = JSONObject(metadataFile.readText())
      Session(
        id = json.getString("id"),
        startedAt = json.getString("started_at"),
        stoppedAt = json.optNullableString("stopped_at"),
        stoppedReason = json.optNullableString("stopped_reason"),
        sampleCount = json.optInt("sample_count", 0).coerceIn(0, MAX_SAMPLES),
        active = json.optBoolean("active", false),
      )
    }
  } catch (_: Throwable) {
    null
  }

  private fun cpuPercent(cpuTimeMs: Long, elapsedMs: Long): Double? {
    val previousCpu = previousCpuTimeMs
    val previousElapsed = previousSampleElapsedMs
    previousCpuTimeMs = cpuTimeMs
    previousSampleElapsedMs = elapsedMs
    if (previousCpu == null || previousElapsed == null || elapsedMs <= previousElapsed) {
      return null
    }
    return (cpuTimeMs - previousCpu).coerceAtLeast(0) * 100.0 / (elapsedMs - previousElapsed)
  }

  private fun elapsedSinceStartMs(current: Session): Long = try {
    (Instant.now().toEpochMilli() - Instant.parse(current.startedAt).toEpochMilli()).coerceAtLeast(0)
  } catch (_: Throwable) {
    0
  }

  private fun lifecycle(importance: Int): String = when {
    importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND -> "foreground"
    importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND_SERVICE -> "background_active"
    importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_VISIBLE -> "visible"
    else -> "background"
  }

  private fun totalPhysicalMemoryBytes(): Long {
    val manager = applicationContext.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
    return ActivityManager.MemoryInfo().also(manager::getMemoryInfo).totalMem
  }

  private fun counterOrNull(value: Long): Long? = value.takeUnless { it == TrafficStats.UNSUPPORTED.toLong() }

  private fun isEmulator(): Boolean =
    Build.FINGERPRINT.startsWith("generic") ||
      Build.FINGERPRINT.contains("emulator") ||
      Build.MODEL.contains("Emulator") ||
      Build.MODEL.contains("Android SDK built for") ||
      Build.MANUFACTURER.contains("Genymotion")

  private fun now(): String = Instant.now().toString()

  private fun JSONObject.putNullable(key: String, value: Any?): JSONObject =
    put(key, value ?: JSONObject.NULL)

  private fun JSONObject.optNullableString(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf { it.isNotEmpty() }

  companion object {
    private const val SAMPLE_INTERVAL_SECONDS = 10L
    private const val MAX_SAMPLES = 8_640
  }
}
