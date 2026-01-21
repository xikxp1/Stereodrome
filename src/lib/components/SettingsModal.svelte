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
  } from "lucide-svelte";
  import {
    getAudioCacheStats,
    clearAudioCache,
    setMaxCacheSize,
    getScanStatus,
    startScan,
    syncLibrary,
    type CacheStats,
  } from "$lib/api/commands";
  import { connection } from "$lib/stores/connection.svelte";
  import { queryClient } from "$lib/db/queryClient";
  import { spectrum } from "$lib/stores/spectrum.svelte";
  import type { ScanStatus } from "$lib/types";

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

  // Cache size in GB for the slider (0.5 to 50 GB)
  let cacheSizeGB = $state(5);

  // Preset size options in GB
  const sizePresets = [0.5, 1, 2, 5, 10, 20, 50];

  // Load stats when modal opens
  $effect(() => {
    if (open) {
      loadCacheStats();
      loadScanStatus();
    } else {
      // Clean up polling when modal closes
      if (scanPollInterval) {
        clearInterval(scanPollInterval);
        scanPollInterval = null;
      }
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
      console.error("Failed to load cache stats:", e);
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
      console.error("Failed to set cache size:", e);
    } finally {
      savingSize = false;
    }
  }

  async function handleClearCache() {
    if (!confirm("Clear all cached audio files? This cannot be undone.")) {
      return;
    }
    clearing = true;
    try {
      await clearAudioCache();
      await loadCacheStats();
    } catch (e) {
      console.error("Failed to clear cache:", e);
    } finally {
      clearing = false;
    }
  }

  async function loadScanStatus() {
    loadingScanStatus = true;
    try {
      scanStatus = await getScanStatus();
    } catch (e) {
      console.error("Failed to load scan status:", e);
    } finally {
      loadingScanStatus = false;
    }
  }

  async function handleStartScan() {
    startingScan = true;
    try {
      scanStatus = await startScan();
    } catch (e) {
      console.error("Failed to start scan:", e);
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
      console.error("Failed to sync library:", e);
    } finally {
      syncing = false;
    }
  }

  async function handleDisconnect() {
    await connection.disconnect();
    onClose();
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
                Sync to Local
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
              <div class="flex items-center gap-2">
                <Activity class="h-4 w-4 text-base-content/60" />
                <span class="text-sm">Spectrum Visualizer</span>
              </div>
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
              Show audio spectrum bars in the transport bar. Press V to toggle.
            </p>
          </div>
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
                <div class="flex flex-wrap gap-1.5">
                  {#each sizePresets as preset (preset)}
                    <button
                      class="btn btn-xs {Math.abs(cacheSizeGB - preset) < 0.01
                        ? 'btn-primary'
                        : 'btn-ghost'}"
                      onclick={() => handleSizeChange(preset)}
                      disabled={savingSize}
                    >
                      {preset < 1 ? `${preset * 1000} MB` : `${preset} GB`}
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
