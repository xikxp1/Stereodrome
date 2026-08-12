<script lang="ts">
  import {
    X,
    RefreshCw,
    Server,
    Database,
    LogOut,
    Monitor,
    Bell,
    Download,
    Clock3,
    Radio,
    ExternalLink,
    ListMusic,
  } from "lucide-svelte";
  import {
    getAudioCacheStats,
    getCacheLocations,
    clearAudioCache,
    setMaxCacheSize,
    getScanStatus,
    startScan,
    syncLibrary,
    getNormalizationSettings,
    setNormalizationSettings,
    getNormalizationStats,
    getAnalysisProgress,
    analyzeAllSongs,
    clearNormalizationData,
    getPlaybackSettings,
    setPlaybackSettings,
    getConnectivitySettings,
    setConnectivitySettings,
    getNotificationSettings,
    setNotificationSettings,
    getSyncSettings,
    setSyncSettings,
    getLibrarySyncStatus,
    reconcileLibraryState,
    getSystemTimePreferences,
    getLastfmStatus,
    beginLastfmAuth,
    completeLastfmAuth,
    disconnectLastfm,
    getLastfmQueue,
    retryLastfmQueue,
    type CacheStats,
  } from "$lib/api/commands";
  import { marked } from "marked";
  import { error } from "@tauri-apps/plugin-log";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { connection } from "$lib/stores/connection.svelte";
  import { updater } from "$lib/stores/updater.svelte";
  import { refreshLibraryViews } from "$lib/services/libraryRefresh.svelte";
  import { spectrum } from "$lib/stores/spectrum.svelte";
  import SettingsAudioSections from "./SettingsAudioSections.svelte";
  import SettingsBackupSection from "./SettingsBackupSection.svelte";
  import {
    EQ_MAX_DB,
    EQ_MIN_DB,
    eqPresets,
    getEqPreset,
    sanitizeEqBands,
    type EqPresetId,
  } from "./settingsAudio";
  import type {
    ScanStatus,
    NormalizationSettings,
    NormalizationStats,
    AnalysisProgress,
    PlaybackSettings,
    NotificationSettings,
    ConnectivitySettings,
    SyncSettings,
    LibrarySyncStatus,
    LastfmQueueItem,
    LastfmStatus,
    CacheLocationInfo,
  } from "$lib/types";

  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();

  // Cache state
  let cacheStats = $state<CacheStats | null>(null);
  let loadingStats = $state(false);
  let clearing = $state(false);
  let savingSize = $state(false);
  let cacheLocations = $state<CacheLocationInfo | null>(null);
  let loadingCacheLocations = $state(false);

  // Scan state
  let scanStatus = $state<ScanStatus | null>(null);
  let loadingScanStatus = $state(false);
  let startingScan = $state(false);
  let syncing = $state(false);
  let reconciling = $state(false);
  let scanPollInterval = $state<ReturnType<typeof setInterval> | null>(null);

  // Playback state
  let playbackSettings = $state<PlaybackSettings | null>(null);
  let connectivitySettings = $state<ConnectivitySettings | null>(null);
  let savingConnectivitySettings = $state(false);
  let notificationSettings = $state<NotificationSettings | null>(null);
  let loadingNotifications = $state(false);
  let savingNotifications = $state(false);

  // Last.fm state
  let lastfmStatus = $state<LastfmStatus | null>(null);
  let lastfmQueue = $state<LastfmQueueItem[]>([]);
  let loadingLastfm = $state(false);
  let startingLastfmAuth = $state(false);
  let completingLastfmAuth = $state(false);
  let disconnectingLastfm = $state(false);
  let retryingLastfmQueue = $state(false);

  // Normalization state
  let normSettings = $state<NormalizationSettings | null>(null);
  let normStats = $state<NormalizationStats | null>(null);
  let loadingNorm = $state(false);
  let savingNorm = $state(false);
  let pendingNormSaves = 0;
  let audioSettingsSaveQueue: Promise<void> = Promise.resolve();
  let analyzing = $state(false);
  let clearingNorm = $state(false);
  let analysisProgress = $state<AnalysisProgress | null>(null);
  let normUnlisten = $state<UnlistenFn | null>(null);

  // Library sync scheduling state
  let syncSettings = $state<SyncSettings | null>(null);
  let syncStatus = $state<LibrarySyncStatus | null>(null);
  let systemLocale = $state<string | null>(null);
  let use24HourClock = $state<boolean | null>(null);
  let loadingSyncSettings = $state(false);
  let loadingSyncStatus = $state(false);
  let savingSyncSettings = $state(false);
  let syncStatusUnlisten = $state<UnlistenFn | null>(null);

  const incrementalIntervals = [5, 15, 30, 60, 180, 360];
  const fullReconcileIntervals = [6, 12, 24, 48, 72, 168];
  const syncDateTimeFormatter = $derived.by(() => {
    const locale = systemLocale ?? undefined;
    const options: Intl.DateTimeFormatOptions = {
      dateStyle: "short",
      timeStyle: "short",
    };
    if (use24HourClock !== null) {
      options.hour12 = !use24HourClock;
    }
    return new Intl.DateTimeFormat(locale, options);
  });

  // Cache size in GB for the slider (0.5 to 50 GB)
  let cacheSizeGB = $state(5);

  // Load stats when modal opens
  $effect(() => {
    if (open) {
      loadCacheStats();
      loadCacheLocations();
      loadScanStatus();
      loadNormalization();
      loadPlaybackSettings();
      loadConnectivitySettings();
      loadNotificationSettings();
      loadLastfm();
      loadSyncSettings();
      loadSyncStatus();
      setupSyncStatusListener();
      loadSystemTimePreferences();
      updater.loadCurrentVersion();
    } else {
      // Clean up polling when modal closes
      if (scanPollInterval) {
        clearInterval(scanPollInterval);
        scanPollInterval = null;
      }
      if (normUnlisten) {
        normUnlisten();
        normUnlisten = null;
      }
      if (syncStatusUnlisten) {
        syncStatusUnlisten();
        syncStatusUnlisten = null;
      }
      // Reset stale state so reopening shows fresh data
      playbackSettings = null;
      connectivitySettings = null;
      notificationSettings = null;
      lastfmStatus = null;
      lastfmQueue = [];
      normSettings = null;
      normStats = null;
      analyzing = false;
      analysisProgress = null;
      syncSettings = null;
      syncStatus = null;
      cacheLocations = null;
    }
  });

  // Poll scan status while scanning
  $effect(() => {
    if (scanStatus?.scanning && !scanPollInterval) {
      scanPollInterval = setInterval(loadScanStatus, 2000);
    } else if (!scanStatus?.scanning && scanPollInterval) {
      clearInterval(scanPollInterval);
      scanPollInterval = null;
    }
  });

  async function loadCacheStats() {
    loadingStats = true;
    try {
      cacheStats = await getAudioCacheStats();
      // Update slider to reflect current max size
      if (cacheStats) {
        cacheSizeGB = cacheStats.max_size / (1024 * 1024 * 1024);
      }
    } catch (e) {
      error(`Failed to load cache stats: ${e}`);
    } finally {
      loadingStats = false;
    }
  }

  async function loadCacheLocations() {
    loadingCacheLocations = true;
    try {
      cacheLocations = await getCacheLocations();
    } catch (e) {
      error(`Failed to load cache locations: ${e}`);
    } finally {
      loadingCacheLocations = false;
    }
  }

  async function handleSizeChange(sizeGB: number) {
    savingSize = true;
    try {
      const sizeBytes = Math.round(sizeGB * 1024 * 1024 * 1024);
      cacheStats = await setMaxCacheSize(sizeBytes);
      cacheSizeGB = sizeGB;
    } catch (e) {
      error(`Failed to set cache size: ${e}`);
    } finally {
      savingSize = false;
    }
  }

  async function handleClearCache() {
    const confirmed = await ask(
      "Clear all cached audio files? This cannot be undone.",
      { title: "Clear Cache", kind: "warning" }
    );
    if (!confirmed) return;
    clearing = true;
    try {
      await clearAudioCache();
      await loadCacheStats();
    } catch (e) {
      error(`Failed to clear cache: ${e}`);
    } finally {
      clearing = false;
    }
  }

  async function handleBackupImported() {
    await Promise.all([
      loadNormalization(),
      loadPlaybackSettings(),
      loadConnectivitySettings(),
      loadSyncSettings(),
      loadSyncStatus(),
    ]);
  }

  async function loadScanStatus() {
    if (connection.manualOfflineEnabled) {
      scanStatus = null;
      return;
    }

    loadingScanStatus = true;
    try {
      scanStatus = await getScanStatus();
    } catch (e) {
      error(`Failed to load scan status: ${e}`);
    } finally {
      loadingScanStatus = false;
    }
  }

  async function handleStartScan() {
    if (connection.manualOfflineEnabled) return;
    startingScan = true;
    try {
      scanStatus = await startScan();
    } catch (e) {
      error(`Failed to start scan: ${e}`);
    } finally {
      startingScan = false;
    }
  }

  async function handleSyncLibrary() {
    if (connection.manualOfflineEnabled) return;
    syncing = true;
    try {
      await syncLibrary();
      await refreshLibraryViews();
    } catch (e) {
      error(`Failed to sync library: ${e}`);
    } finally {
      syncing = false;
      await loadSyncStatus();
    }
  }

  async function handleReconcileLibrary() {
    if (connection.manualOfflineEnabled) return;
    reconciling = true;
    try {
      await reconcileLibraryState();
      await refreshLibraryViews();
    } catch (e) {
      error(`Failed to reconcile library: ${e}`);
    } finally {
      reconciling = false;
      await loadSyncStatus();
    }
  }

  async function handleDisconnect() {
    await connection.disconnect();
    onClose();
  }

  async function loadConnectivitySettings() {
    try {
      connectivitySettings = await getConnectivitySettings();
    } catch (e) {
      error(`Failed to load connectivity settings: ${e}`);
    }
  }

  async function handleConnectivitySettingChange(
    update: Partial<ConnectivitySettings>
  ) {
    if (!connectivitySettings) return;
    savingConnectivitySettings = true;
    try {
      const updated = { ...connectivitySettings, ...update };
      connectivitySettings = await setConnectivitySettings(updated);
      await connection.checkStatus();
    } catch (e) {
      error(`Failed to save connectivity settings: ${e}`);
    } finally {
      savingConnectivitySettings = false;
    }
  }

  async function loadPlaybackSettings() {
    try {
      const settings = await getPlaybackSettings();
      playbackSettings = {
        ...settings,
        equalizer_bands_db: sanitizeEqBands(settings.equalizer_bands_db),
      };
    } catch (e) {
      error(`Failed to load playback settings: ${e}`);
    }
  }

  function enqueueAudioSettingsSave(save: () => Promise<void>) {
    const queuedSave = audioSettingsSaveQueue.then(save);
    audioSettingsSaveQueue = queuedSave.catch(() => undefined);
    return queuedSave;
  }

  async function handlePlaybackSettingChange(
    update: Partial<PlaybackSettings>
  ) {
    if (!playbackSettings) return;
    try {
      const updated = {
        ...playbackSettings,
        ...update,
        equalizer_bands_db: sanitizeEqBands(
          update.equalizer_bands_db ?? playbackSettings.equalizer_bands_db
        ),
      };
      playbackSettings = updated;
      await enqueueAudioSettingsSave(() => setPlaybackSettings(updated));
    } catch (e) {
      error(`Failed to save playback settings: ${e}`);
    }
  }

  async function loadNotificationSettings() {
    loadingNotifications = true;
    try {
      notificationSettings = await getNotificationSettings();
    } catch (e) {
      error(`Failed to load notification settings: ${e}`);
    } finally {
      loadingNotifications = false;
    }
  }

  async function handleNotificationSettingChange(
    update: Partial<NotificationSettings>
  ) {
    if (!notificationSettings) return;
    savingNotifications = true;
    try {
      const updated = { ...notificationSettings, ...update };
      await setNotificationSettings(updated);
      notificationSettings = updated;
    } catch (e) {
      error(`Failed to save notification settings: ${e}`);
    } finally {
      savingNotifications = false;
    }
  }

  async function loadLastfm() {
    loadingLastfm = true;
    try {
      const [status, queue] = await Promise.all([
        getLastfmStatus(),
        getLastfmQueue(),
      ]);
      lastfmStatus = status;
      lastfmQueue = queue;
    } catch (e) {
      error(`Failed to load Last.fm status: ${e}`);
    } finally {
      loadingLastfm = false;
    }
  }

  async function handleBeginLastfmAuth() {
    startingLastfmAuth = true;
    try {
      const auth = await beginLastfmAuth();
      await openUrl(auth.auth_url);
      await loadLastfm();
    } catch (e) {
      error(`Failed to start Last.fm authorization: ${e}`);
    } finally {
      startingLastfmAuth = false;
    }
  }

  async function handleCompleteLastfmAuth() {
    completingLastfmAuth = true;
    try {
      lastfmStatus = await completeLastfmAuth();
      lastfmQueue = await getLastfmQueue();
    } catch (e) {
      error(`Failed to complete Last.fm authorization: ${e}`);
    } finally {
      completingLastfmAuth = false;
    }
  }

  async function handleDisconnectLastfm() {
    disconnectingLastfm = true;
    try {
      lastfmStatus = await disconnectLastfm();
    } catch (e) {
      error(`Failed to disconnect Last.fm: ${e}`);
    } finally {
      disconnectingLastfm = false;
    }
  }

  async function handleRetryLastfmQueue() {
    retryingLastfmQueue = true;
    try {
      await retryLastfmQueue();
      await loadLastfm();
    } catch (e) {
      error(`Failed to retry Last.fm queue: ${e}`);
      await loadLastfm();
    } finally {
      retryingLastfmQueue = false;
    }
  }

  async function setupSyncStatusListener() {
    if (syncStatusUnlisten) return;
    try {
      syncStatusUnlisten = await listen<LibrarySyncStatus>(
        "library-sync-status-changed",
        (event) => {
          syncStatus = event.payload;
        }
      );
    } catch (e) {
      error(`Failed to attach library sync status listener: ${e}`);
    }
  }

  async function loadSyncSettings() {
    loadingSyncSettings = true;
    try {
      syncSettings = await getSyncSettings();
    } catch (e) {
      error(`Failed to load sync settings: ${e}`);
    } finally {
      loadingSyncSettings = false;
    }
  }

  async function loadSyncStatus() {
    loadingSyncStatus = true;
    try {
      syncStatus = await getLibrarySyncStatus();
    } catch (e) {
      error(`Failed to load sync status: ${e}`);
    } finally {
      loadingSyncStatus = false;
    }
  }

  async function loadSystemTimePreferences() {
    try {
      const preferences = await getSystemTimePreferences();
      use24HourClock = preferences.use_24_hour_clock;
      systemLocale = preferences.locale;
    } catch (e) {
      error(`Failed to load system time preferences: ${e}`);
      use24HourClock = null;
      systemLocale = null;
    }
  }

  async function handleSyncSettingChange(update: Partial<SyncSettings>) {
    if (!syncSettings) return;
    savingSyncSettings = true;
    try {
      const updated = { ...syncSettings, ...update };
      await setSyncSettings(updated);
      syncSettings = updated;
      await loadSyncStatus();
    } catch (e) {
      error(`Failed to save sync settings: ${e}`);
    } finally {
      savingSyncSettings = false;
    }
  }

  function formatSyncTimestamp(value: string | null | undefined): string {
    if (!value) return "Never";
    const parsed = new Date(value);
    if (Number.isNaN(parsed.getTime())) return "Invalid date";
    return syncDateTimeFormatter.format(parsed);
  }

  function formatNextSync(value: string | null | undefined): string {
    if (!value) return "Disabled";
    const parsed = new Date(value);
    if (Number.isNaN(parsed.getTime())) return "Invalid date";
    return syncDateTimeFormatter.format(parsed);
  }

  function formatUnixTimestamp(value: number | null | undefined): string {
    if (!value || value <= 0) return "Now";
    return syncDateTimeFormatter.format(new Date(value * 1000));
  }

  function syncJobLabel(job: string | null | undefined): string {
    if (!job) return "Idle";
    return job === "incremental"
      ? "Running incremental sync"
      : "Running full reconcile";
  }

  const activeEqPreset = $derived.by(() => {
    if (!playbackSettings) return null;
    return getEqPreset(playbackSettings.equalizer_bands_db);
  });

  const activeEqDescription = $derived.by(() => {
    if (!activeEqPreset) {
      return "Custom EQ curve.";
    }
    return (
      eqPresets.find((preset) => preset.id === activeEqPreset)?.description ??
      "Custom EQ curve."
    );
  });

  function formatDb(value: number): string {
    return `${value >= 0 ? "+" : ""}${value.toFixed(1)} dB`;
  }

  function getEqBandValue(index: number): number {
    return playbackSettings
      ? (sanitizeEqBands(playbackSettings.equalizer_bands_db)[index] ?? 0)
      : 0;
  }

  function previewEqBand(index: number, value: number) {
    if (!playbackSettings) return;
    const bands = sanitizeEqBands(playbackSettings.equalizer_bands_db);
    bands[index] = Math.min(EQ_MAX_DB, Math.max(EQ_MIN_DB, value));
    playbackSettings = {
      ...playbackSettings,
      equalizer_bands_db: bands,
    };
  }

  async function commitEqBand(index: number, value: number) {
    if (!playbackSettings) return;
    const bands = sanitizeEqBands(playbackSettings.equalizer_bands_db);
    bands[index] = Math.min(EQ_MAX_DB, Math.max(EQ_MIN_DB, value));
    await handlePlaybackSettingChange({ equalizer_bands_db: bands });
  }

  async function applyEqPreset(presetId: EqPresetId) {
    const preset = eqPresets.find((entry) => entry.id === presetId);
    if (!preset) return;
    await handlePlaybackSettingChange({
      equalizer_enabled: true,
      equalizer_bands_db: [...preset.bands],
    });
  }

  async function loadNormalization() {
    loadingNorm = true;
    try {
      normSettings = await getNormalizationSettings();
      normStats = await getNormalizationStats();
      // Check if analysis is already in progress
      const currentProgress = await getAnalysisProgress();
      if (currentProgress && normStats) {
        analyzing = true;
        analysisProgress = currentProgress;
        normStats.analyzed_count = currentProgress.analyzed_count;
        normStats.total_count = currentProgress.total_count;
      }
      // Always listen for progress events while modal is open
      // (analysis may already be running from a previous session)
      if (!normUnlisten) {
        normUnlisten = await listen<AnalysisProgress>(
          "normalization-progress",
          (event) => {
            analysisProgress = event.payload;
            analyzing = true;
            if (normStats) {
              normStats.analyzed_count = event.payload.analyzed_count;
              normStats.total_count = event.payload.total_count;
            }
            if (
              event.payload.analyzed >= event.payload.total &&
              event.payload.total > 0
            ) {
              analyzing = false;
              analysisProgress = null;
              loadNormalization();
            }
          }
        );
      }
    } catch (e) {
      error(`Failed to load normalization settings: ${e}`);
    } finally {
      loadingNorm = false;
    }
  }

  async function handleNormSettingChange(
    update: Partial<NormalizationSettings>
  ) {
    if (!normSettings) return;
    const updated = { ...normSettings, ...update };
    normSettings = updated;
    pendingNormSaves += 1;
    savingNorm = true;
    try {
      await enqueueAudioSettingsSave(() => setNormalizationSettings(updated));
    } catch (e) {
      error(`Failed to save normalization settings: ${e}`);
    } finally {
      pendingNormSaves -= 1;
      savingNorm = pendingNormSaves > 0;
    }
  }

  async function handleAnalyzeAll() {
    analyzing = true;
    analysisProgress = null;
    try {
      await analyzeAllSongs();
    } catch (e) {
      error(`Failed to start analysis: ${e}`);
      analyzing = false;
    }
  }

  async function handleClearNormData() {
    const confirmed = await ask(
      "Clear all normalization data? Songs will be re-analyzed.",
      { title: "Clear Data", kind: "warning" }
    );
    if (!confirmed) return;
    clearingNorm = true;
    try {
      await clearNormalizationData();
      await loadNormalization();
    } catch (e) {
      error(`Failed to clear normalization data: ${e}`);
    } finally {
      clearingNorm = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onClose();
    }
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onClose();
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
    role="dialog"
    aria-modal="true"
    aria-labelledby="settings-title"
    tabindex="-1"
    onclick={handleBackdropClick}
    onkeydown={handleKeydown}
  >
    <div
      class="w-full max-w-md rounded-lg border border-base-300 bg-base-100 shadow-xl"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
      role="document"
    >
      <!-- Header -->
      <div
        class="flex items-center justify-between border-b border-base-300 px-4 py-3"
      >
        <h2 id="settings-title" class="text-lg font-semibold">Settings</h2>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded text-base-content/60 transition-colors hover:bg-base-200 hover:text-base-content"
          onclick={onClose}
          aria-label="Close settings"
        >
          <X class="h-4 w-4" />
        </button>
      </div>

      <!-- Content -->
      <div class="max-h-[70vh] space-y-4 overflow-y-auto p-4">
        <!-- Updates Section -->
        <div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
          <div class="mb-3 flex items-center gap-2">
            <Download class="h-4 w-4 text-base-content/60" />
            <h3 class="font-medium">Updates</h3>
          </div>

          <div class="space-y-2">
            <div class="flex justify-between text-sm">
              <span class="text-base-content/60">Current version</span>
              <span class="font-medium">
                {updater.currentVersion ?? "-"}
              </span>
            </div>

            {#if updater.updateAvailable}
              <div class="flex justify-between text-sm">
                <span class="text-base-content/60">Available version</span>
                <span class="font-medium text-success">
                  {updater.version}
                </span>
              </div>
              {#if updater.notes}
                <div
                  class="prose prose-xs mt-2 max-h-48 max-w-none overflow-y-auto rounded border border-base-300 bg-base-300/50 p-2 text-base-content/70"
                >
                  <!-- eslint-disable-next-line svelte/no-at-html-tags -- Trusted content from app updater -->
                  {@html marked.parse(updater.notes, { async: false })}
                </div>
              {/if}
            {/if}

            {#if updater.error}
              <div class="text-sm text-error">
                {updater.error}
              </div>
            {/if}
          </div>

          <div class="mt-4 flex flex-wrap gap-2 border-t border-base-300 pt-4">
            <button
              type="button"
              class="btn btn-sm btn-ghost gap-1"
              onclick={() => updater.checkForUpdate()}
              disabled={updater.checking}
            >
              {#if updater.checking}
                <RefreshCw class="h-3.5 w-3.5 animate-spin" />
                Checking...
              {:else}
                <RefreshCw class="h-3.5 w-3.5" />
                Check for Updates
              {/if}
            </button>
            {#if updater.updateAvailable}
              <button
                type="button"
                class="btn btn-sm btn-primary gap-1"
                onclick={() => updater.downloadAndInstall()}
                disabled={updater.downloading}
              >
                {#if updater.downloading}
                  <Download class="h-3.5 w-3.5 animate-pulse" />
                  Installing...
                {:else}
                  <Download class="h-3.5 w-3.5" />
                  Install Update
                {/if}
              </button>
            {/if}
          </div>
        </div>

        <!-- Server Section -->
        <div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
          <div class="mb-3 flex items-center gap-2">
            <Server class="h-4 w-4 text-base-content/60" />
            <h3 class="font-medium">Server</h3>
          </div>

          <div class="space-y-2">
            <div class="flex justify-between text-sm">
              <span class="text-base-content/60">URL</span>
              <span class="max-w-48 truncate font-medium">
                {connection.status.server_url ?? "Not connected"}
              </span>
            </div>
            <div class="flex justify-between text-sm">
              <span class="text-base-content/60">Username</span>
              <span class="font-medium">
                {connection.status.username ?? "-"}
              </span>
            </div>
            <div class="flex justify-between text-sm">
              <span class="text-base-content/60">Server version</span>
              <span class="font-medium">
                {connection.status.server_version ?? "-"}
              </span>
            </div>
            <label
              class="flex cursor-pointer items-center justify-between text-sm"
            >
              <span class="text-base-content/60">Offline mode</span>
              <input
                type="checkbox"
                class="checkbox checkbox-sm checkbox-primary"
                checked={connectivitySettings?.manual_offline_enabled ?? false}
                onchange={(e) =>
                  handleConnectivitySettingChange({
                    manual_offline_enabled: e.currentTarget.checked,
                  })}
                disabled={!connectivitySettings || savingConnectivitySettings}
              />
            </label>
            <div class="flex justify-between text-sm">
              <span class="text-base-content/60">Scan status</span>
              <span class="font-medium">
                {#if loadingScanStatus && !scanStatus}
                  <RefreshCw class="inline h-3 w-3 animate-spin" />
                {:else if scanStatus?.scanning}
                  <span class="text-warning">
                    Scanning{scanStatus.count
                      ? ` (${scanStatus.count} items)`
                      : "..."}
                  </span>
                {:else if scanStatus}
                  <span class="text-success">Idle</span>
                  {#if scanStatus.count}
                    <span class="text-base-content/50">
                      ({scanStatus.count.toLocaleString()} items)
                    </span>
                  {/if}
                {:else}
                  -
                {/if}
              </span>
            </div>
          </div>

          <div class="mt-4 flex flex-wrap gap-2 border-t border-base-300 pt-4">
            <button
              type="button"
              class="btn btn-sm btn-ghost gap-1"
              onclick={handleStartScan}
              disabled={connection.manualOfflineEnabled ||
                startingScan ||
                scanStatus?.scanning}
            >
              {#if startingScan || scanStatus?.scanning}
                <RefreshCw class="h-3.5 w-3.5 animate-spin" />
                {scanStatus?.scanning ? "Scanning..." : "Starting..."}
              {:else}
                <RefreshCw class="h-3.5 w-3.5" />
                Start Scan
              {/if}
            </button>
            <button
              type="button"
              class="btn btn-sm btn-error btn-outline gap-1"
              onclick={handleDisconnect}
            >
              <LogOut class="h-3.5 w-3.5" />
              Disconnect
            </button>
          </div>
        </div>

        <!-- Library Sync Section -->
        <div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
          <div class="mb-3 flex items-center gap-2">
            <Clock3 class="h-4 w-4 text-base-content/60" />
            <h3 class="font-medium">Library Sync</h3>
          </div>

          {#if (loadingSyncSettings && !syncSettings) || (loadingSyncStatus && !syncStatus)}
            <div class="flex items-center gap-2 text-sm text-base-content/60">
              <RefreshCw class="h-4 w-4 animate-spin" />
              Loading...
            </div>
          {:else if syncSettings && syncStatus}
            <div class="space-y-3">
              <div
                class="rounded border border-base-300 bg-base-100/60 px-3 py-2"
              >
                <div class="flex items-center justify-between text-sm">
                  <span class="text-base-content/60">Scheduler</span>
                  <span
                    class={syncStatus.active_job
                      ? "font-medium text-warning"
                      : "font-medium text-success"}
                  >
                    {syncJobLabel(syncStatus.active_job)}
                  </span>
                </div>
              </div>

              <div class="border-t border-base-300 pt-3">
                <label class="flex cursor-pointer items-center justify-between">
                  <span class="text-sm">Periodic incremental sync</span>
                  <input
                    type="checkbox"
                    class="checkbox checkbox-sm checkbox-primary"
                    checked={syncSettings.incremental_enabled}
                    onchange={(e) =>
                      handleSyncSettingChange({
                        incremental_enabled: e.currentTarget.checked,
                      })}
                    disabled={connection.manualOfflineEnabled ||
                      savingSyncSettings}
                  />
                </label>

                {#if syncSettings.incremental_enabled}
                  <div class="mt-2 flex flex-wrap gap-1">
                    {#each incrementalIntervals as interval (interval)}
                      <button
                        type="button"
                        class="btn btn-xs h-6 min-h-0 px-2 {syncSettings.incremental_interval_minutes ===
                        interval
                          ? 'btn-primary'
                          : 'btn-ghost'}"
                        onclick={() =>
                          handleSyncSettingChange({
                            incremental_interval_minutes: interval,
                          })}
                        disabled={connection.manualOfflineEnabled ||
                          savingSyncSettings}
                      >
                        {interval >= 60 ? `${interval / 60}h` : `${interval}m`}
                      </button>
                    {/each}
                  </div>
                {/if}

                <div class="mt-2 space-y-1 text-xs text-base-content/55">
                  <div>
                    Next: {formatNextSync(syncStatus.incremental.next_run_at)}
                  </div>
                  <div>
                    Last success: {formatSyncTimestamp(
                      syncStatus.incremental.last_success_at
                    )}
                  </div>
                  {#if syncStatus.incremental.last_error}
                    <div class="text-error">
                      Last error: {syncStatus.incremental.last_error}
                    </div>
                  {/if}
                </div>
              </div>

              <div class="border-t border-base-300 pt-3">
                <label class="flex cursor-pointer items-center justify-between">
                  <span class="text-sm">Periodic full reconcile</span>
                  <input
                    type="checkbox"
                    class="checkbox checkbox-sm checkbox-primary"
                    checked={syncSettings.full_reconcile_enabled}
                    onchange={(e) =>
                      handleSyncSettingChange({
                        full_reconcile_enabled: e.currentTarget.checked,
                      })}
                    disabled={connection.manualOfflineEnabled ||
                      savingSyncSettings}
                  />
                </label>

                {#if syncSettings.full_reconcile_enabled}
                  <div class="mt-2 flex flex-wrap gap-1">
                    {#each fullReconcileIntervals as interval (interval)}
                      <button
                        type="button"
                        class="btn btn-xs h-6 min-h-0 px-2 {syncSettings.full_reconcile_interval_hours ===
                        interval
                          ? 'btn-primary'
                          : 'btn-ghost'}"
                        onclick={() =>
                          handleSyncSettingChange({
                            full_reconcile_interval_hours: interval,
                          })}
                        disabled={connection.manualOfflineEnabled ||
                          savingSyncSettings}
                      >
                        {interval}h
                      </button>
                    {/each}
                  </div>
                {/if}

                <div class="mt-2 space-y-1 text-xs text-base-content/55">
                  <div>
                    Next: {formatNextSync(
                      syncStatus.full_reconcile.next_run_at
                    )}
                  </div>
                  <div>
                    Last success: {formatSyncTimestamp(
                      syncStatus.full_reconcile.last_success_at
                    )}
                  </div>
                  {#if syncStatus.full_reconcile.last_error}
                    <div class="text-error">
                      Last error: {syncStatus.full_reconcile.last_error}
                    </div>
                  {/if}
                </div>
              </div>
            </div>

            <div
              class="mt-4 flex flex-wrap gap-2 border-t border-base-300 pt-4"
            >
              <button
                type="button"
                class="btn btn-sm btn-ghost gap-1"
                onclick={handleSyncLibrary}
                disabled={connection.manualOfflineEnabled ||
                  syncing ||
                  Boolean(syncStatus.active_job)}
              >
                {#if syncing}
                  <Database class="h-3.5 w-3.5 animate-pulse" />
                  Syncing...
                {:else}
                  <Database class="h-3.5 w-3.5" />
                  Run Incremental Now
                {/if}
              </button>
              <button
                type="button"
                class="btn btn-sm btn-ghost gap-1"
                onclick={handleReconcileLibrary}
                disabled={connection.manualOfflineEnabled ||
                  reconciling ||
                  Boolean(syncStatus.active_job)}
              >
                {#if reconciling}
                  <RefreshCw class="h-3.5 w-3.5 animate-spin" />
                  Reconciling...
                {:else}
                  <RefreshCw class="h-3.5 w-3.5" />
                  Run Full Reconcile
                {/if}
              </button>
            </div>
          {:else}
            <div class="text-sm text-base-content/60">
              Unable to load library sync settings
            </div>
          {/if}
        </div>

        <SettingsBackupSection onImported={handleBackupImported} />

        <!-- Display Section -->
        <div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
          <div class="mb-3 flex items-center gap-2">
            <Monitor class="h-4 w-4 text-base-content/60" />
            <h3 class="font-medium">Display</h3>
          </div>

          <div class="space-y-3">
            <label class="flex cursor-pointer items-center justify-between">
              <span class="text-sm">Spectrum Visualizer</span>
              <input
                type="checkbox"
                class="checkbox checkbox-sm checkbox-primary"
                checked={spectrum.enabled}
                onchange={(e) => {
                  if (e.currentTarget.checked !== spectrum.enabled) {
                    spectrum.toggle();
                  }
                }}
              />
            </label>
            <p class="text-xs text-base-content/50">
              Show audio spectrum bars in Now Playing. Press V to toggle.
            </p>

            {#if playbackSettings}
              <div class="border-t border-base-300 pt-3"></div>

              <label class="flex cursor-pointer items-center justify-between">
                <span class="text-sm">Mini Player shows Next song</span>
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm checkbox-primary"
                  checked={playbackSettings.show_next_song_in_miniplayer}
                  onchange={(e) =>
                    handlePlaybackSettingChange({
                      show_next_song_in_miniplayer: e.currentTarget.checked,
                    })}
                />
              </label>
              <p class="text-xs text-base-content/50">
                On: second line shows upcoming Next track. Off: show Artist —
                Album.
              </p>
            {/if}
          </div>
        </div>

        <!-- Notifications Section -->
        <div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
          <div class="mb-3 flex items-center gap-2">
            <Bell class="h-4 w-4 text-base-content/60" />
            <h3 class="font-medium">Desktop Notifications</h3>
          </div>

          {#if loadingNotifications && !notificationSettings}
            <div class="flex items-center gap-2 text-sm text-base-content/60">
              <RefreshCw class="h-4 w-4 animate-spin" />
              Loading...
            </div>
          {:else if notificationSettings}
            <div class="space-y-3">
              <label class="flex cursor-pointer items-center justify-between">
                <span class="text-sm">Enable</span>
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm checkbox-primary"
                  checked={notificationSettings.enabled}
                  onchange={(e) =>
                    handleNotificationSettingChange({
                      enabled: e.currentTarget.checked,
                    })}
                  disabled={savingNotifications}
                />
              </label>

              {#if notificationSettings.enabled}
                <div class="border-t border-base-300 pt-3"></div>

                <label class="flex cursor-pointer items-center justify-between">
                  <span class="text-sm">Notify when app is focused</span>
                  <input
                    type="checkbox"
                    class="checkbox checkbox-sm checkbox-primary"
                    checked={notificationSettings.notify_when_focused}
                    onchange={(e) =>
                      handleNotificationSettingChange({
                        notify_when_focused: e.currentTarget.checked,
                      })}
                    disabled={savingNotifications}
                  />
                </label>
                <p class="text-xs text-base-content/50">
                  Show notifications while app is focused.
                </p>

                <label class="flex cursor-pointer items-center justify-between">
                  <span class="text-sm">Notify when mini player is open</span>
                  <input
                    type="checkbox"
                    class="checkbox checkbox-sm checkbox-primary"
                    checked={notificationSettings.notify_when_miniplayer_open}
                    onchange={(e) =>
                      handleNotificationSettingChange({
                        notify_when_miniplayer_open: e.currentTarget.checked,
                      })}
                    disabled={savingNotifications}
                  />
                </label>
                <p class="text-xs text-base-content/50">
                  Show notifications while the mini player window is visible.
                </p>
              {/if}
            </div>
          {:else}
            <div class="text-sm text-base-content/60">
              Unable to load notification settings
            </div>
          {/if}
        </div>

        <!-- Last.fm Section -->
        <div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
          <div class="mb-3 flex items-center gap-2">
            <Radio class="h-4 w-4 text-base-content/60" />
            <h3 class="font-medium">Last.fm</h3>
          </div>

          {#if loadingLastfm && !lastfmStatus}
            <div class="flex items-center gap-2 text-sm text-base-content/60">
              <RefreshCw class="h-4 w-4 animate-spin" />
              Loading...
            </div>
          {:else if lastfmStatus}
            <div class="space-y-3">
              <div class="space-y-2">
                <div class="flex justify-between gap-4 text-sm">
                  <span class="text-base-content/60">Status</span>
                  <span
                    class={lastfmStatus.available && lastfmStatus.authenticated
                      ? "font-medium text-success"
                      : lastfmStatus.pending_auth
                        ? "font-medium text-warning"
                        : "font-medium text-base-content/70"}
                  >
                    {#if !lastfmStatus.available}
                      Not configured
                    {:else if lastfmStatus.authenticated}
                      Connected
                    {:else if lastfmStatus.pending_auth}
                      Authorization pending
                    {:else}
                      Disconnected
                    {/if}
                  </span>
                </div>

                {#if lastfmStatus.username}
                  <div class="flex justify-between gap-4 text-sm">
                    <span class="text-base-content/60">Account</span>
                    <span class="max-w-48 truncate font-medium">
                      {lastfmStatus.username}
                    </span>
                  </div>
                {/if}

                <div class="flex justify-between gap-4 text-sm">
                  <span class="text-base-content/60">Queued scrobbles</span>
                  <span class="font-medium">
                    {lastfmStatus.queue_count.toLocaleString()}
                  </span>
                </div>

                {#if lastfmStatus.last_error}
                  <div
                    class="rounded border border-error/30 bg-error/10 p-2 text-xs text-error"
                  >
                    {lastfmStatus.last_error}
                  </div>
                {/if}
              </div>

              {#if lastfmQueue.length > 0}
                <div class="border-t border-base-300 pt-3">
                  <div class="mb-2 flex items-center gap-2 text-sm">
                    <ListMusic class="h-3.5 w-3.5 text-base-content/60" />
                    <span class="text-base-content/60">Pending queue</span>
                  </div>
                  <div class="max-h-40 space-y-2 overflow-y-auto pr-1">
                    {#each lastfmQueue.slice(0, 6) as item (item.id)}
                      <div
                        class="rounded border border-base-300 bg-base-100/60 px-3 py-2"
                      >
                        <div class="truncate text-sm font-medium">
                          {item.title}
                        </div>
                        <div class="truncate text-xs text-base-content/60">
                          {item.artist}{item.album ? ` — ${item.album}` : ""}
                        </div>
                        <div
                          class="mt-1 flex justify-between gap-3 text-[0.68rem] text-base-content/45"
                        >
                          <span>{formatUnixTimestamp(item.played_at)}</span>
                          <span>
                            {item.attempts > 0
                              ? `${item.attempts} attempts`
                              : "Not retried"}
                          </span>
                        </div>
                        {#if item.last_error}
                          <div
                            class="mt-1 line-clamp-2 text-[0.68rem] text-error"
                          >
                            {item.last_error}
                          </div>
                        {:else if item.next_retry_at > 0}
                          <div class="mt-1 text-[0.68rem] text-base-content/45">
                            Next retry: {formatUnixTimestamp(
                              item.next_retry_at
                            )}
                          </div>
                        {/if}
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}

              <div
                class="mt-4 flex flex-wrap gap-2 border-t border-base-300 pt-4"
              >
                {#if lastfmStatus.available && !lastfmStatus.authenticated}
                  <button
                    type="button"
                    class="btn btn-sm btn-primary gap-1"
                    onclick={handleBeginLastfmAuth}
                    disabled={connection.manualOfflineEnabled ||
                      startingLastfmAuth}
                  >
                    {#if startingLastfmAuth}
                      <RefreshCw class="h-3.5 w-3.5 animate-spin" />
                      Opening...
                    {:else}
                      <ExternalLink class="h-3.5 w-3.5" />
                      Connect
                    {/if}
                  </button>
                {/if}

                {#if lastfmStatus.pending_auth}
                  <button
                    type="button"
                    class="btn btn-sm btn-ghost gap-1"
                    onclick={handleCompleteLastfmAuth}
                    disabled={connection.manualOfflineEnabled ||
                      completingLastfmAuth}
                  >
                    {#if completingLastfmAuth}
                      <RefreshCw class="h-3.5 w-3.5 animate-spin" />
                      Completing...
                    {:else}
                      <RefreshCw class="h-3.5 w-3.5" />
                      Complete
                    {/if}
                  </button>
                {/if}

                {#if lastfmStatus.authenticated}
                  <button
                    type="button"
                    class="btn btn-sm btn-ghost gap-1"
                    onclick={handleRetryLastfmQueue}
                    disabled={connection.manualOfflineEnabled ||
                      retryingLastfmQueue ||
                      lastfmStatus.queue_count === 0}
                  >
                    {#if retryingLastfmQueue}
                      <RefreshCw class="h-3.5 w-3.5 animate-spin" />
                      Retrying...
                    {:else}
                      <RefreshCw class="h-3.5 w-3.5" />
                      Retry Now
                    {/if}
                  </button>
                  <button
                    type="button"
                    class="btn btn-sm btn-error btn-outline gap-1"
                    onclick={handleDisconnectLastfm}
                    disabled={disconnectingLastfm}
                  >
                    <LogOut class="h-3.5 w-3.5" />
                    {disconnectingLastfm ? "Disconnecting..." : "Disconnect"}
                  </button>
                {/if}
              </div>
            </div>
          {:else}
            <div class="text-sm text-base-content/60">
              Unable to load Last.fm status
            </div>
          {/if}
        </div>

        <SettingsAudioSections
          {playbackSettings}
          {handlePlaybackSettingChange}
          {activeEqPreset}
          {activeEqDescription}
          {applyEqPreset}
          {formatDb}
          {getEqBandValue}
          {previewEqBand}
          {commitEqBand}
          {loadingNorm}
          {normSettings}
          {handleNormSettingChange}
          {savingNorm}
          {normStats}
          {analyzing}
          {analysisProgress}
          {handleAnalyzeAll}
          {clearingNorm}
          {handleClearNormData}
          {loadingStats}
          {cacheStats}
          {loadingCacheLocations}
          {cacheLocations}
          {cacheSizeGB}
          {savingSize}
          {handleSizeChange}
          {loadCacheStats}
          {clearing}
          {handleClearCache}
        />
      </div>
    </div>
  </div>
{/if}
