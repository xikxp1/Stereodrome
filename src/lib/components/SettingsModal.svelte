<script lang="ts">
  import {
    X,
    HardDrive,
    Trash2,
    RefreshCw,
    Server,
    Database,
    LogOut,
    Monitor,
    Activity,
    Download,
    Volume2,
    ShieldCheck,
    Disc3,
  } from "lucide-svelte";
  import {
    getAudioCacheStats,
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
    type CacheStats,
  } from "$lib/api/commands";
  import { marked } from "marked";
  import { error } from "@tauri-apps/plugin-log";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { connection } from "$lib/stores/connection.svelte";
  import { updater } from "$lib/stores/updater.svelte";
  import { queryClient } from "$lib/db/queryClient";
  import { spectrum } from "$lib/stores/spectrum.svelte";
  import type {
    ScanStatus,
    NormalizationSettings,
    NormalizationStats,
    AnalysisProgress,
    BinauralPreset,
    DynamicsPreset,
    PlaybackSettings,
  } from "$lib/types";

  const dynamicsPresets: DynamicsPreset[] = ["light", "medium", "heavy"];
  const dynamicsDescriptions: Record<DynamicsPreset, string> = {
    light: "Gentle compression. Preserves dynamics.",
    medium: "Balanced compression for mixed playlists.",
    heavy: "Strong compression. Maximum consistency.",
  };
  const binauralPresets: BinauralPreset[] = [
    "default",
    "cmoy",
    "jmeier",
    "aggressive",
  ];
  const binauralDescriptions: Record<BinauralPreset, string> = {
    default: "Moderate crossfeed (700Hz / 4.5dB).",
    cmoy: "Subtle crossffed that matches famous Chu Moy analog circuit.",
    jmeier: "Jan Meier profile with the most subtle crossfeed effect.",
    aggressive: "Max-strength crossfeed for obvious A/B testing.",
  };

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

  // Scan state
  let scanStatus = $state<ScanStatus | null>(null);
  let loadingScanStatus = $state(false);
  let startingScan = $state(false);
  let syncing = $state(false);
  let scanPollInterval = $state<ReturnType<typeof setInterval> | null>(null);

  // Playback state
  let playbackSettings = $state<PlaybackSettings | null>(null);

  // Normalization state
  let normSettings = $state<NormalizationSettings | null>(null);
  let normStats = $state<NormalizationStats | null>(null);
  let loadingNorm = $state(false);
  let savingNorm = $state(false);
  let analyzing = $state(false);
  let clearingNorm = $state(false);
  let analysisProgress = $state<AnalysisProgress | null>(null);
  let normUnlisten = $state<UnlistenFn | null>(null);

  const lufsPresets = [-18, -16, -14, -12, -10];

  // Cache size in GB for the slider (0.5 to 50 GB)
  let cacheSizeGB = $state(5);

  // Preset size options in GB
  const sizePresets = [0.5, 1, 2, 5, 10, 20, 50];

  // Load stats when modal opens
  $effect(() => {
    if (open) {
      loadCacheStats();
      loadScanStatus();
      loadNormalization();
      loadPlaybackSettings();
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
      // Reset stale state so reopening shows fresh data
      playbackSettings = null;
      normSettings = null;
      normStats = null;
      analyzing = false;
      analysisProgress = null;
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

  async function loadScanStatus() {
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
    syncing = true;
    try {
      await syncLibrary();
      await queryClient.invalidateQueries({ queryKey: ["artists"] });
      await queryClient.invalidateQueries({ queryKey: ["albums"] });
      await queryClient.invalidateQueries({ queryKey: ["songs"] });
    } catch (e) {
      error(`Failed to sync library: ${e}`);
    } finally {
      syncing = false;
    }
  }

  async function handleDisconnect() {
    await connection.disconnect();
    onClose();
  }

  async function loadPlaybackSettings() {
    try {
      playbackSettings = await getPlaybackSettings();
    } catch (e) {
      error(`Failed to load playback settings: ${e}`);
    }
  }

  async function handlePlaybackSettingChange(
    update: Partial<PlaybackSettings>
  ) {
    if (!playbackSettings) return;
    try {
      const updated = { ...playbackSettings, ...update };
      await setPlaybackSettings(updated);
      playbackSettings = updated;
    } catch (e) {
      error(`Failed to save playback settings: ${e}`);
    }
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
    savingNorm = true;
    try {
      const updated = { ...normSettings, ...update };
      await setNormalizationSettings(updated);
      normSettings = updated;
    } catch (e) {
      error(`Failed to save normalization settings: ${e}`);
    } finally {
      savingNorm = false;
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

  function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
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
              class="btn btn-sm btn-ghost gap-1"
              onclick={handleStartScan}
              disabled={startingScan || scanStatus?.scanning}
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
              class="btn btn-sm btn-ghost gap-1"
              onclick={handleSyncLibrary}
              disabled={syncing}
            >
              {#if syncing}
                <Database class="h-3.5 w-3.5 animate-pulse" />
                Syncing...
              {:else}
                <Database class="h-3.5 w-3.5" />
                Sync Library
              {/if}
            </button>
            <button
              class="btn btn-sm btn-error btn-outline gap-1"
              onclick={handleDisconnect}
            >
              <LogOut class="h-3.5 w-3.5" />
              Disconnect
            </button>
          </div>
        </div>

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
          </div>
        </div>

        <!-- Playback Section -->
        <div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
          <div class="mb-3 flex items-center gap-2">
            <Disc3 class="h-4 w-4 text-base-content/60" />
            <h3 class="font-medium">Playback</h3>
          </div>

          {#if playbackSettings}
            <div class="space-y-3">
              <label class="flex cursor-pointer items-center justify-between">
                <span class="text-sm">Gapless playback</span>
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm checkbox-primary"
                  checked={playbackSettings.gapless_enabled}
                  onchange={(e) =>
                    handlePlaybackSettingChange({
                      gapless_enabled: e.currentTarget.checked,
                    })}
                />
              </label>
              <p class="text-xs text-base-content/50">
                Seamless transitions between consecutive album tracks.
              </p>

              <div class="border-t border-base-300 pt-3"></div>

              <label class="flex cursor-pointer items-center justify-between">
                <span class="text-sm">Crossfade</span>
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm checkbox-primary"
                  checked={playbackSettings.crossfade_enabled}
                  onchange={(e) =>
                    handlePlaybackSettingChange({
                      crossfade_enabled: e.currentTarget.checked,
                    })}
                />
              </label>
              <p class="text-xs text-base-content/50">
                Smooth volume transitions between non-album tracks.
              </p>

              {#if playbackSettings.crossfade_enabled}
                <div>
                  <div class="mb-2 flex items-center justify-between text-sm">
                    <span class="text-base-content/60">Duration</span>
                    <span class="text-xs text-base-content/50"
                      >{(playbackSettings.crossfade_duration_ms / 1000).toFixed(
                        0
                      )}s</span
                    >
                  </div>
                  <div class="flex flex-wrap gap-1">
                    {#each [1000, 3000, 5000, 8000, 12000] as duration (duration)}
                      <button
                        class="btn btn-xs h-6 min-h-0 px-2 {playbackSettings.crossfade_duration_ms ===
                        duration
                          ? 'btn-primary'
                          : 'btn-ghost'}"
                        onclick={() =>
                          handlePlaybackSettingChange({
                            crossfade_duration_ms: duration,
                          })}
                      >
                        {duration / 1000}s
                      </button>
                    {/each}
                  </div>
                </div>
              {/if}

              <div class="border-t border-base-300 pt-3"></div>

              <label class="flex cursor-pointer items-center justify-between">
                <span class="text-sm">Binaural crossfeed</span>
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm checkbox-primary"
                  checked={playbackSettings.binaural_enabled}
                  onchange={(e) =>
                    handlePlaybackSettingChange({
                      binaural_enabled: e.currentTarget.checked,
                    })}
                />
              </label>
              <p class="text-xs text-base-content/50">
                Convert stereo to binaural-style headphone playback with bs2b.
              </p>

              {#if playbackSettings.binaural_enabled}
                <div>
                  <div class="mb-2 text-sm text-base-content/60">Preset</div>
                  <div class="flex flex-wrap gap-1">
                    {#each binauralPresets as preset (preset)}
                      <button
                        class="btn btn-xs h-6 min-h-0 px-3 {playbackSettings.binaural_preset ===
                        preset
                          ? 'btn-primary'
                          : 'btn-ghost'}"
                        onclick={() =>
                          handlePlaybackSettingChange({
                            binaural_preset: preset,
                          })}
                      >
                        {preset === "default"
                          ? "Default"
                          : preset === "cmoy"
                            ? "C-Moy"
                            : preset === "jmeier"
                              ? "JMeier"
                              : "Aggressive"}
                      </button>
                    {/each}
                  </div>
                  <p class="mt-1 text-xs text-base-content/50">
                    {binauralDescriptions[playbackSettings.binaural_preset]}
                  </p>
                </div>
              {/if}
            </div>
          {/if}
        </div>

        <!-- Volume Normalization Section -->
        <div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
          <div class="mb-3 flex items-center gap-2">
            <Volume2 class="h-4 w-4 text-base-content/60" />
            <h3 class="font-medium">Volume Normalization</h3>
          </div>

          {#if loadingNorm}
            <div class="flex items-center gap-2 text-sm text-base-content/60">
              <RefreshCw class="h-4 w-4 animate-spin" />
              Loading...
            </div>
          {:else if normSettings}
            <div class="space-y-3">
              <!-- Enable toggle -->
              <label class="flex cursor-pointer items-center justify-between">
                <span class="text-sm">Enable normalization</span>
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm checkbox-primary"
                  checked={normSettings.enabled}
                  onchange={(e) =>
                    handleNormSettingChange({
                      enabled: e.currentTarget.checked,
                    })}
                />
              </label>

              {#if normSettings.enabled}
                <!-- Mode selection -->
                <div>
                  <div class="mb-1 text-sm text-base-content/60">Mode</div>
                  <div class="flex gap-3">
                    <label class="flex cursor-pointer items-center gap-1.5">
                      <input
                        type="radio"
                        name="norm-mode"
                        class="radio radio-sm radio-primary"
                        checked={normSettings.mode === "track"}
                        onchange={() =>
                          handleNormSettingChange({ mode: "track" })}
                      />
                      <span class="text-sm">Track</span>
                    </label>
                    <label class="flex cursor-pointer items-center gap-1.5">
                      <input
                        type="radio"
                        name="norm-mode"
                        class="radio radio-sm radio-primary"
                        checked={normSettings.mode === "album"}
                        onchange={() =>
                          handleNormSettingChange({ mode: "album" })}
                      />
                      <span class="text-sm">Album</span>
                    </label>
                  </div>
                  <p class="mt-1 text-xs text-base-content/50">
                    {normSettings.mode === "album"
                      ? "Preserves relative dynamics within albums."
                      : "Each track normalized independently."}
                  </p>
                </div>

                <!-- Target LUFS presets -->
                <div>
                  <div class="mb-2 flex items-center justify-between text-sm">
                    <span class="text-base-content/60">Target level</span>
                    <span class="text-xs text-base-content/50"
                      >{normSettings.target_lufs} LUFS</span
                    >
                  </div>
                  <div class="flex flex-wrap gap-1">
                    {#each lufsPresets as preset (preset)}
                      <button
                        class="btn btn-xs h-6 min-h-0 px-2 {normSettings.target_lufs ===
                        preset
                          ? 'btn-primary'
                          : 'btn-ghost'}"
                        onclick={() =>
                          handleNormSettingChange({ target_lufs: preset })}
                        disabled={savingNorm}
                      >
                        {preset}
                      </button>
                    {/each}
                  </div>
                </div>

                <!-- Pre-amp slider -->
                <div>
                  <div class="mb-1 flex items-center justify-between text-sm">
                    <span class="text-base-content/60">Pre-amp</span>
                    <span class="text-xs text-base-content/50"
                      >{normSettings.pre_amp_db > 0
                        ? "+"
                        : ""}{normSettings.pre_amp_db.toFixed(1)} dB</span
                    >
                  </div>
                  <input
                    type="range"
                    min="-6"
                    max="6"
                    step="0.5"
                    class="preamp-slider w-full"
                    value={normSettings.pre_amp_db}
                    oninput={(e) => {
                      if (normSettings)
                        normSettings.pre_amp_db = parseFloat(
                          e.currentTarget.value
                        );
                    }}
                    onchange={(e) =>
                      handleNormSettingChange({
                        pre_amp_db: parseFloat(e.currentTarget.value),
                      })}
                  />
                </div>

                <!-- Prevent clipping -->
                <label class="flex cursor-pointer items-center justify-between">
                  <span class="text-sm">Prevent clipping</span>
                  <input
                    type="checkbox"
                    class="checkbox checkbox-sm checkbox-primary"
                    checked={normSettings.prevent_clipping}
                    onchange={(e) =>
                      handleNormSettingChange({
                        prevent_clipping: e.currentTarget.checked,
                      })}
                  />
                </label>

                <!-- Dynamics processing -->
                <label class="flex cursor-pointer items-center justify-between">
                  <span class="text-sm">Dynamics processing</span>
                  <input
                    type="checkbox"
                    class="checkbox checkbox-sm checkbox-primary"
                    checked={normSettings.dynamics_enabled}
                    onchange={(e) =>
                      handleNormSettingChange({
                        dynamics_enabled: e.currentTarget.checked,
                      })}
                  />
                </label>

                {#if normSettings.dynamics_enabled}
                  <div>
                    <div class="mb-2 text-sm text-base-content/60">Amount</div>
                    <div class="flex flex-wrap gap-1">
                      {#each dynamicsPresets as preset (preset)}
                        <button
                          class="btn btn-xs h-6 min-h-0 px-3 {normSettings.dynamics_preset ===
                          preset
                            ? 'btn-primary'
                            : 'btn-ghost'}"
                          onclick={() =>
                            handleNormSettingChange({
                              dynamics_preset: preset,
                            })}
                          disabled={savingNorm}
                        >
                          {preset.charAt(0).toUpperCase() + preset.slice(1)}
                        </button>
                      {/each}
                    </div>
                    <p class="mt-1 text-xs text-base-content/50">
                      {dynamicsDescriptions[normSettings.dynamics_preset]}
                    </p>
                  </div>
                {/if}
              {/if}

              <!-- Analysis stats -->
              {#if normStats}
                <div class="border-t border-base-300 pt-3">
                  <div class="flex justify-between text-sm">
                    <span class="text-base-content/60">Songs analyzed</span>
                    <span class="font-medium">
                      {normStats.analyzed_count.toLocaleString()} / {normStats.total_count.toLocaleString()}
                    </span>
                  </div>

                  <!-- Progress bar -->
                  <div class="mt-2">
                    <div
                      class="h-2 w-full overflow-hidden rounded-full bg-base-300"
                    >
                      <div
                        class="h-full bg-primary transition-all"
                        style="width: {normStats.total_count > 0
                          ? Math.min(
                              100,
                              (normStats.analyzed_count /
                                normStats.total_count) *
                                100
                            )
                          : 0}%"
                      ></div>
                    </div>
                    {#if analyzing && analysisProgress}
                      <div class="mt-1 text-right text-xs text-base-content/50">
                        Analyzing... {analysisProgress.analyzed} / {analysisProgress.total}
                      </div>
                    {:else}
                      <div class="mt-1 text-right text-xs text-base-content/50">
                        {normStats.total_count > 0
                          ? (
                              (normStats.analyzed_count /
                                normStats.total_count) *
                              100
                            ).toFixed(1)
                          : "0.0"}% analyzed
                      </div>
                    {/if}
                  </div>

                  <div class="mt-3 flex gap-2">
                    <button
                      class="btn btn-sm btn-ghost gap-1"
                      onclick={handleAnalyzeAll}
                      disabled={analyzing ||
                        normStats.analyzed_count >= normStats.total_count}
                    >
                      {#if analyzing}
                        <RefreshCw class="h-3.5 w-3.5 animate-spin" />
                        Analyzing...
                      {:else}
                        <RefreshCw class="h-3.5 w-3.5" />
                        Analyze All
                      {/if}
                    </button>
                    <button
                      class="btn btn-sm btn-error btn-outline gap-1"
                      onclick={handleClearNormData}
                      disabled={clearingNorm || normStats.analyzed_count === 0}
                    >
                      <Trash2 class="h-3.5 w-3.5" />
                      {clearingNorm ? "Clearing..." : "Clear Data"}
                    </button>
                  </div>
                </div>
              {/if}
            </div>
          {:else}
            <div class="text-sm text-base-content/60">
              Unable to load normalization settings
            </div>
          {/if}
        </div>

        <!-- Audio Cache Section -->
        <div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
          <div class="mb-3 flex items-center gap-2">
            <HardDrive class="h-4 w-4 text-base-content/60" />
            <h3 class="font-medium">Audio Cache</h3>
          </div>

          {#if loadingStats}
            <div class="flex items-center gap-2 text-sm text-base-content/60">
              <RefreshCw class="h-4 w-4 animate-spin" />
              Loading...
            </div>
          {:else if cacheStats}
            <div class="mb-4 space-y-2">
              <div class="flex justify-between text-sm">
                <span class="text-base-content/60">Cached files</span>
                <span class="font-medium">{cacheStats.file_count}</span>
              </div>
              <div class="flex justify-between text-sm">
                <span class="text-base-content/60">Used space</span>
                <span class="font-medium"
                  >{formatBytes(cacheStats.total_size)}</span
                >
              </div>
              <div class="flex justify-between text-sm">
                <span class="text-base-content/60">Maximum size</span>
                <span class="font-medium"
                  >{formatBytes(cacheStats.max_size)}</span
                >
              </div>
              <!-- Progress bar -->
              <div class="mt-2">
                <div
                  class="h-2 w-full overflow-hidden rounded-full bg-base-300"
                >
                  <div
                    class="h-full bg-primary transition-all"
                    style="width: {Math.min(
                      100,
                      (cacheStats.total_size / cacheStats.max_size) * 100
                    )}%"
                  ></div>
                </div>
                <div class="mt-1 text-right text-xs text-base-content/50">
                  {(
                    (cacheStats.total_size / cacheStats.max_size) *
                    100
                  ).toFixed(1)}% used
                </div>
              </div>

              <!-- Cache size presets -->
              <div class="mt-4 border-t border-base-300 pt-4">
                <div class="mb-2 flex items-center justify-between text-sm">
                  <span class="text-base-content/60">Maximum cache size</span>
                  {#if savingSize}
                    <RefreshCw class="h-3.5 w-3.5 animate-spin" />
                  {/if}
                </div>
                <div class="flex flex-wrap gap-1">
                  {#each sizePresets as preset (preset)}
                    <button
                      class="btn btn-xs h-6 min-h-0 px-2 {Math.abs(
                        cacheSizeGB - preset
                      ) < 0.01
                        ? 'btn-primary'
                        : 'btn-ghost'}"
                      onclick={() => handleSizeChange(preset)}
                      disabled={savingSize}
                    >
                      {preset < 1 ? `${preset * 1000}MB` : `${preset}GB`}
                    </button>
                  {/each}
                </div>
              </div>
            </div>

            <div class="flex gap-2">
              <button
                class="btn btn-sm btn-ghost gap-1"
                onclick={loadCacheStats}
                disabled={loadingStats}
              >
                <RefreshCw
                  class="h-3.5 w-3.5 {loadingStats ? 'animate-spin' : ''}"
                />
                Refresh
              </button>
              <button
                class="btn btn-sm btn-error btn-outline gap-1"
                onclick={handleClearCache}
                disabled={clearing || cacheStats.file_count === 0}
              >
                <Trash2 class="h-3.5 w-3.5" />
                {clearing ? "Clearing..." : "Clear Cache"}
              </button>
            </div>
          {:else}
            <div class="text-sm text-base-content/60">
              Unable to load cache statistics
            </div>
          {/if}
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .preamp-slider {
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    border-radius: 2px;
    background: oklch(0.3 0 0);
    outline: none;
    cursor: pointer;
  }

  .preamp-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: oklch(0.58 0.2 250);
    cursor: pointer;
  }
</style>
