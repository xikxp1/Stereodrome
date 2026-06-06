<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { error } from "@tauri-apps/plugin-log";
  import { Database, RefreshCw } from "lucide-svelte";
  import {
    getLibrarySyncStatus,
    getSystemTimePreferences,
    reconcileLibraryState,
    syncLibrary,
  } from "$lib/api/commands";
  import { refreshLibraryViews } from "$lib/services/libraryRefresh.svelte";
  import { connection } from "$lib/stores/connection.svelte";
  import { playlistStore } from "$lib/stores/playlist.svelte";
  import type { LibrarySyncStatus, SyncJobKind } from "$lib/types";

  interface Props {
    itemCount?: number;
    totalCount?: number;
    totalDuration?: number;
    totalSize?: number;
    itemType?: "songs" | "artists" | "albums";
  }

  let {
    itemCount = 0,
    totalCount = 0,
    totalDuration = 0,
    totalSize = 0,
    itemType = "songs",
  }: Props = $props();

  let librarySyncStatus = $state<LibrarySyncStatus | null>(null);
  let loadingSyncStatus = $state(false);
  let syncPopoverOpen = $state(false);
  let runningManualJob = $state<SyncJobKind | null>(null);
  let syncActionError = $state<string | null>(null);
  let systemLocale = $state<string | null>(null);
  let use24HourClock = $state<boolean | null>(null);

  let syncControlRef: HTMLDivElement | null = null;
  let timePreferencesLoaded = false;

  function formatDuration(seconds: number): string {
    if (!seconds) return "0 minutes";
    const hours = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);

    if (hours > 0) {
      return `${hours}.${Math.floor(mins / 6)} hours`;
    }
    return `${mins} minutes`;
  }

  function formatSize(bytes: number): string {
    if (!bytes) return "0 MB";
    const mb = bytes / (1024 * 1024);
    if (mb >= 1024) {
      return `${(mb / 1024).toFixed(1)} GB`;
    }
    return `${mb.toFixed(1)} MB`;
  }

  function getItemLabel(count: number, type: string): string {
    if (count === 1) {
      return type === "songs"
        ? "song"
        : type === "artists"
          ? "artist"
          : "album";
    }
    return type;
  }

  async function loadLibrarySyncStatus() {
    if (!connection.status.connected || connection.manualOfflineEnabled) return;
    loadingSyncStatus = true;
    try {
      librarySyncStatus = await getLibrarySyncStatus();
    } catch (e) {
      error(`Failed to load library sync status: ${e}`);
    } finally {
      loadingSyncStatus = false;
    }
  }

  async function refreshConnectionStatus() {
    if (connection.isConnecting || connection.isInitializing) return;

    try {
      await connection.checkStatus();
    } catch {
      // ConnectionStore already captures the error state.
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

  async function runSyncJob(job: SyncJobKind) {
    runningManualJob = job;
    syncActionError = null;

    try {
      if (connection.manualOfflineEnabled) return;
      if (job === "incremental") {
        await syncLibrary();
      } else {
        await reconcileLibraryState();
      }

      await refreshLibraryViews();
      await playlistStore.syncPlaylists();
    } catch (e) {
      syncActionError = e instanceof Error ? e.message : String(e);
    } finally {
      runningManualJob = null;
      await loadLibrarySyncStatus();
    }
  }

  function toggleSyncPopover() {
    syncPopoverOpen = !syncPopoverOpen;
    syncActionError = null;
    if (syncPopoverOpen) {
      void loadLibrarySyncStatus();
    }
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && syncPopoverOpen) {
      syncPopoverOpen = false;
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

  function syncJobLabel(job: SyncJobKind | null | undefined): string {
    if (!job) return "Idle";
    return job === "incremental"
      ? "Running incremental sync"
      : "Running full sync";
  }

  $effect(() => {
    if (timePreferencesLoaded) return;
    timePreferencesLoaded = true;
    void loadSystemTimePreferences();
  });

  $effect(() => {
    let unlistenStatus: UnlistenFn | null = null;
    let unlistenSettings: UnlistenFn | null = null;

    async function subscribeToSyncEvents() {
      try {
        unlistenStatus = await listen<LibrarySyncStatus>(
          "library-sync-status-changed",
          (event) => {
            librarySyncStatus = event.payload;
          }
        );
      } catch (e) {
        error(`Failed to listen for library sync status events: ${e}`);
      }

      try {
        unlistenSettings = await listen("sync-settings-changed", () => {
          void loadLibrarySyncStatus();
        });
      } catch (e) {
        error(`Failed to listen for sync settings events: ${e}`);
      }
    }

    void subscribeToSyncEvents();

    return () => {
      if (unlistenStatus) unlistenStatus();
      if (unlistenSettings) unlistenSettings();
    };
  });

  $effect(() => {
    if (!connection.status.connected || connection.manualOfflineEnabled) {
      librarySyncStatus = null;
      syncPopoverOpen = false;
      return;
    }

    void loadLibrarySyncStatus();
    const interval = setInterval(() => {
      void loadLibrarySyncStatus();
    }, 60_000);

    return () => {
      clearInterval(interval);
    };
  });

  $effect(() => {
    if (!syncPopoverOpen) return;

    const handleDocumentClick = (event: MouseEvent) => {
      if (!(event.target instanceof Node)) return;
      if (syncControlRef?.contains(event.target)) return;
      syncPopoverOpen = false;
    };

    document.addEventListener("click", handleDocumentClick, true);
    return () => {
      document.removeEventListener("click", handleDocumentClick, true);
    };
  });

  $effect(() => {
    void refreshConnectionStatus();

    const interval = window.setInterval(() => {
      void refreshConnectionStatus();
    }, 10_000);

    return () => {
      window.clearInterval(interval);
    };
  });

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

  const statusText = $derived.by(() => {
    const parts: string[] = [];

    if (itemCount > 0) {
      if (totalCount > itemCount) {
        parts.push(
          `${itemCount} of ${totalCount} ${getItemLabel(totalCount, itemType)}`
        );
      } else {
        parts.push(`${itemCount} ${getItemLabel(itemCount, itemType)}`);
      }
    }

    if (totalDuration > 0) {
      parts.push(formatDuration(totalDuration));
    }

    if (totalSize > 0) {
      parts.push(formatSize(totalSize));
    }

    return parts.join(", ") || `No ${itemType}`;
  });

  const serverLabel = $derived.by(() => {
    const serverUrl = connection.status.server_url;
    if (!serverUrl) return "Server";
    return serverUrl.replace(/^https?:\/\//i, "");
  });

  const connectionBadge = $derived.by(() => {
    if (connection.isConnecting || connection.isInitializing) {
      return {
        label: "Connecting",
        toneClass: "text-warning",
        dotClass: "bg-warning",
        pulse: true,
      };
    }

    if (!connection.status.server_url) {
      return {
        label: "No server",
        toneClass: "text-base-content/45",
        dotClass: "bg-base-content/35",
        pulse: false,
      };
    }

    if (connection.manualOfflineEnabled) {
      return {
        label: "Offline",
        toneClass: "text-info",
        dotClass: "bg-info",
        pulse: false,
      };
    }

    if (connection.status.connected) {
      return {
        label: "Online",
        toneClass: "text-success",
        dotClass: "bg-success",
        pulse: false,
      };
    }

    return {
      label: "Issue",
      toneClass: "text-error",
      dotClass: "bg-error",
      pulse: true,
    };
  });

  const runningIndicatorLabel = $derived.by(() => {
    const activeJob = librarySyncStatus?.active_job ?? runningManualJob;
    if (activeJob === "incremental") return "Inc";
    if (activeJob === "full_reconcile") return "Full";
    return null;
  });

  const hasSyncError = $derived.by(() =>
    Boolean(
      librarySyncStatus?.incremental.last_error ||
      librarySyncStatus?.full_reconcile.last_error
    )
  );

  const hasActiveSyncJob = $derived.by(
    () => Boolean(librarySyncStatus?.active_job) || runningManualJob !== null
  );

  const controlToneClass = $derived.by(() => {
    if (hasActiveSyncJob) {
      return "text-warning/90";
    }
    if (hasSyncError) {
      return "text-error/90";
    }
    return "opacity-60";
  });

  const incrementalBusy = $derived.by(
    () =>
      runningManualJob === "incremental" ||
      librarySyncStatus?.active_job === "incremental"
  );

  const fullBusy = $derived.by(
    () =>
      runningManualJob === "full_reconcile" ||
      librarySyncStatus?.active_job === "full_reconcile"
  );
</script>

<svelte:window
  onkeydown={handleWindowKeydown}
  onfocus={refreshConnectionStatus}
/>

<div
  class="relative h-6 flex items-center justify-between gap-3 px-4 select-none bg-base-200 border-t border-base-300"
>
  <span class="text-xs opacity-60 truncate min-w-0">{statusText}</span>

  <div bind:this={syncControlRef} class="relative shrink-0 max-w-[55%]">
    <button
      class={`flex max-w-full items-center justify-end gap-2 text-xs transition-colors hover:opacity-100 ${controlToneClass}`}
      onclick={toggleSyncPopover}
      title={`${connection.status.server_url ?? serverLabel} • ${connectionBadge.label}`}
      type="button"
    >
      <span class="truncate">{serverLabel}</span>
      <span
        class={`inline-flex items-center gap-1 rounded-full border border-current/20 px-1.5 py-0.5 text-[10px] font-medium ${connectionBadge.toneClass}`}
      >
        <span
          class={`size-1.5 rounded-full ${connectionBadge.dotClass} ${connectionBadge.pulse ? "animate-pulse" : ""}`}
        ></span>
        {connectionBadge.label}
      </span>
      {#if runningIndicatorLabel}
        <span
          class="inline-flex items-center gap-1 rounded-full border border-current/20 px-1.5 py-0.5 text-[10px] font-medium"
        >
          <span class="size-1.5 rounded-full bg-current animate-pulse"></span>
          {runningIndicatorLabel}
        </span>
      {/if}
    </button>

    {#if syncPopoverOpen}
      <div
        class="absolute bottom-full right-0 z-20 mb-2 w-80 rounded-lg border border-base-300 bg-base-100 p-3 text-left shadow-xl"
      >
        <div class="flex items-center">
          <div class="min-w-0">
            <div
              class="text-[11px] uppercase tracking-[0.08em] text-base-content/45"
            >
              Server
            </div>
            <div
              class="flex items-center gap-2 text-sm font-medium text-base-content"
            >
              <span class="truncate">{serverLabel}</span>
              <span
                class={`inline-flex shrink-0 items-center gap-1 rounded-full border border-current/20 px-1.5 py-0.5 text-[10px] font-medium ${connectionBadge.toneClass}`}
              >
                <span
                  class={`size-1.5 rounded-full ${connectionBadge.dotClass} ${connectionBadge.pulse ? "animate-pulse" : ""}`}
                ></span>
                {connectionBadge.label}
              </span>
            </div>
          </div>
        </div>

        {#if loadingSyncStatus && !librarySyncStatus}
          <div
            class="mt-3 flex items-center gap-2 text-sm text-base-content/60"
          >
            <RefreshCw class="size-4 animate-spin" />
            Loading sync status...
          </div>
        {:else if librarySyncStatus}
          <div class="mt-3 space-y-3">
            <div
              class="rounded border border-base-300 bg-base-200/60 px-3 py-2"
            >
              <div class="flex items-center justify-between gap-3 text-sm">
                <span class="text-base-content/60">Scheduler</span>
                <span
                  class={hasActiveSyncJob
                    ? "font-medium text-warning"
                    : "font-medium text-success"}
                >
                  {syncJobLabel(librarySyncStatus.active_job)}
                </span>
              </div>
            </div>

            <div class="space-y-3 text-sm">
              <section class="rounded border border-base-300 px-3 py-2">
                <div class="mb-2 flex items-center justify-between gap-3">
                  <span class="font-medium">Incremental sync</span>
                  <span
                    class={librarySyncStatus.incremental.running
                      ? "text-warning"
                      : "text-base-content/50"}
                  >
                    {librarySyncStatus.incremental.running ? "Running" : "Idle"}
                  </span>
                </div>
                <div class="space-y-1 text-xs text-base-content/55">
                  <div>
                    Next: {formatNextSync(
                      librarySyncStatus.incremental.next_run_at
                    )}
                  </div>
                  <div>
                    Last success: {formatSyncTimestamp(
                      librarySyncStatus.incremental.last_success_at
                    )}
                  </div>
                  {#if librarySyncStatus.incremental.last_error}
                    <div class="text-error">
                      Last error: {librarySyncStatus.incremental.last_error}
                    </div>
                  {/if}
                </div>
                <button
                  class="btn btn-xs mt-3 h-7 min-h-0 gap-1.5"
                  onclick={() => runSyncJob("incremental")}
                  disabled={hasActiveSyncJob}
                  type="button"
                >
                  {#if incrementalBusy}
                    <Database class="size-3.5 animate-pulse" />
                    Running...
                  {:else}
                    <Database class="size-3.5" />
                    Run Incremental Now
                  {/if}
                </button>
              </section>

              <section class="rounded border border-base-300 px-3 py-2">
                <div class="mb-2 flex items-center justify-between gap-3">
                  <span class="font-medium">Full sync</span>
                  <span
                    class={librarySyncStatus.full_reconcile.running
                      ? "text-warning"
                      : "text-base-content/50"}
                  >
                    {librarySyncStatus.full_reconcile.running
                      ? "Running"
                      : "Idle"}
                  </span>
                </div>
                <div class="space-y-1 text-xs text-base-content/55">
                  <div>
                    Next: {formatNextSync(
                      librarySyncStatus.full_reconcile.next_run_at
                    )}
                  </div>
                  <div>
                    Last success: {formatSyncTimestamp(
                      librarySyncStatus.full_reconcile.last_success_at
                    )}
                  </div>
                  {#if librarySyncStatus.full_reconcile.last_error}
                    <div class="text-error">
                      Last error: {librarySyncStatus.full_reconcile.last_error}
                    </div>
                  {/if}
                </div>
                <button
                  class="btn btn-xs mt-3 h-7 min-h-0 gap-1.5"
                  onclick={() => runSyncJob("full_reconcile")}
                  disabled={hasActiveSyncJob}
                  type="button"
                >
                  {#if fullBusy}
                    <RefreshCw class="size-3.5 animate-spin" />
                    Running...
                  {:else}
                    <RefreshCw class="size-3.5" />
                    Run Full Sync
                  {/if}
                </button>
              </section>
            </div>

            {#if syncActionError}
              <div
                class="rounded border border-error/30 bg-error/8 px-3 py-2 text-xs text-error"
              >
                {syncActionError}
              </div>
            {/if}
          </div>
        {:else}
          <div class="mt-3 text-sm text-base-content/60">
            Sync status unavailable.
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
