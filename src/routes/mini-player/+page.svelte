<script lang="ts">
  import NowPlayingCenter from "$lib/components/NowPlayingCenter.svelte";
  import {
    closeMiniPlayer as closeMiniPlayerCommand,
    getCoverArt,
    restoreMainWindow as restoreMainWindowCommand,
    setMiniPlayerPosition,
    seekPlayback,
  } from "$lib/api/commands";
  import { playback } from "$lib/stores/playback.svelte";
  import { queue } from "$lib/stores/queue.svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { error } from "@tauri-apps/plugin-log";
  import { onMount } from "svelte";
  import { GripHorizontal, MonitorUp, X } from "lucide-svelte";
  import type { MiniPlayerHoverState } from "$lib/types";

  const POSITION_PERSIST_DEBOUNCE_MS = 250;

  const currentTrack = $derived(playback.currentTrack);

  let coverArtUrl = $state<string | null>(null);
  let lastCoverArtId = $state<string | null>(null);
  let closing = $state(false);
  let isHovered = $state(false);
  let persistedMiniPlayerX = $state<number | null>(null);
  let persistedMiniPlayerY = $state<number | null>(null);
  let persistPositionTimeout = $state<ReturnType<typeof setTimeout> | null>(
    null
  );

  // Reveal controls immediately on hover; force-hide them when window blurs.
  const miniControlsVisible = $derived(isHovered);

  $effect(() => {
    const coverArtId = currentTrack?.coverArtId;
    if (coverArtId && coverArtId !== lastCoverArtId) {
      lastCoverArtId = coverArtId;
      getCoverArt(coverArtId, 96)
        .then((url) => {
          coverArtUrl = url;
        })
        .catch((e) => {
          error(`Failed to fetch mini player cover art: ${e}`);
          coverArtUrl = null;
        });
    } else if (!coverArtId) {
      coverArtUrl = null;
      lastCoverArtId = null;
    }
  });

  function handleSeek(position: number) {
    void seekPlayback(position);
  }

  async function handleDragStart() {
    try {
      await getCurrentWindow().startDragging();
      // Some platforms/window types do not emit moved events consistently while dragging.
      // Persist once at drag end so position updates are never missed.
      await captureAndScheduleCurrentWindowPosition();
    } catch (e) {
      error(`Failed to start mini player drag: ${e}`);
    }
  }

  async function restoreMainWindow() {
    try {
      await restoreMainWindowCommand();
    } catch (e) {
      error(`Failed to restore main window: ${e}`);
    }
  }

  function latestPersistedMiniPlayerPosition(): {
    x: number;
    y: number;
  } | null {
    if (persistedMiniPlayerX === null || persistedMiniPlayerY === null) {
      return null;
    }

    // Return a plain object so invoke payload is never a reactive proxy.
    return {
      x: persistedMiniPlayerX,
      y: persistedMiniPlayerY,
    };
  }

  async function persistMiniPlayerPosition(position: {
    x: number;
    y: number;
  }): Promise<void> {
    await setMiniPlayerPosition({
      x: position.x,
      y: position.y,
    });
  }

  function schedulePositionPersistence(position: { x: number; y: number }) {
    persistedMiniPlayerX = position.x;
    persistedMiniPlayerY = position.y;
    if (persistPositionTimeout) {
      clearTimeout(persistPositionTimeout);
    }

    persistPositionTimeout = setTimeout(() => {
      const latestPosition = latestPersistedMiniPlayerPosition();
      if (!latestPosition) return;
      void persistMiniPlayerPosition(latestPosition).catch((e) => {
        error(`Failed to persist mini player position: ${e}`);
      });
      persistPositionTimeout = null;
    }, POSITION_PERSIST_DEBOUNCE_MS);
  }

  async function persistCurrentWindowPosition(): Promise<void> {
    const currentWindow = getCurrentWindow();
    const [scale, outerPosition] = await Promise.all([
      currentWindow.scaleFactor(),
      currentWindow.outerPosition(),
    ]);
    await persistMiniPlayerPosition({
      x: outerPosition.x / scale,
      y: outerPosition.y / scale,
    });
  }

  async function captureAndScheduleCurrentWindowPosition(): Promise<void> {
    const currentWindow = getCurrentWindow();
    const [scale, outerPosition] = await Promise.all([
      currentWindow.scaleFactor(),
      currentWindow.outerPosition(),
    ]);
    schedulePositionPersistence({
      x: outerPosition.x / scale,
      y: outerPosition.y / scale,
    });
  }

  async function closeMiniPlayer() {
    if (closing) return;
    closing = true;

    try {
      if (persistPositionTimeout) {
        clearTimeout(persistPositionTimeout);
        persistPositionTimeout = null;
      }
      await persistCurrentWindowPosition();
      await closeMiniPlayerCommand();
    } catch (e) {
      error(`Failed to close mini player window: ${e}`);
    } finally {
      // The mini-player window is hidden (not destroyed), so this component
      // instance survives across reopen. Reset the guard for future closes.
      closing = false;
    }
  }

  onMount(() => {
    const currentWindow = getCurrentWindow();

    const closeRequestedPromise = currentWindow.onCloseRequested((event) => {
      if (closing) {
        return;
      }
      event.preventDefault();
      void closeMiniPlayer();
    });

    const movedPromise = currentWindow.onMoved((event) => {
      void (async () => {
        try {
          const scale = await currentWindow.scaleFactor();
          schedulePositionPersistence({
            x: event.payload.x / scale,
            y: event.payload.y / scale,
          });
        } catch (e) {
          error(`Failed to persist mini player position: ${e}`);
        }
      })();
    });

    const focusChangedPromise = currentWindow.onFocusChanged((event) => {
      if (!event.payload) {
        isHovered = false;
      }
    });

    const hoverChangedPromise = listen<MiniPlayerHoverState>(
      "mini-player-hover-state",
      (event) => {
        isHovered = event.payload.hovered;
      }
    );

    return () => {
      closeRequestedPromise.then((unlisten) => unlisten());
      movedPromise.then((unlisten) => unlisten());
      focusChangedPromise.then((unlisten) => unlisten());
      hoverChangedPromise.then((unlisten) => unlisten());
      if (persistPositionTimeout) {
        clearTimeout(persistPositionTimeout);
        persistPositionTimeout = null;
        const latestPosition = latestPersistedMiniPlayerPosition();
        if (latestPosition) {
          void persistMiniPlayerPosition(latestPosition).catch((e) => {
            error(`Failed to persist mini player position: ${e}`);
          });
        }
      }
    };
  });
</script>

<svelte:head>
  <title>Stereodrome Mini Player</title>
</svelte:head>

<div
  class="group/mini relative flex h-screen w-screen items-center overflow-hidden bg-linear-to-b from-base-200 to-base-300 px-0.5 py-0 shadow-lg"
  role="group"
  onpointerenter={() => {
    isHovered = true;
  }}
  onpointerleave={() => {
    isHovered = false;
  }}
>
  <div
    class={`absolute right-1 top-1 z-30 flex items-center gap-1 transition-opacity ${
      miniControlsVisible
        ? "opacity-100 pointer-events-auto"
        : "opacity-0 pointer-events-none"
    }`}
  >
    <button
      type="button"
      class="flex h-5 w-5 items-center justify-center rounded bg-base-100/80 text-base-content/70 transition-all hover:scale-105 hover:bg-base-100 hover:text-base-content active:scale-95"
      onmousedown={handleDragStart}
      aria-label="Drag mini player"
      title="Drag mini player"
    >
      <GripHorizontal class="h-3 w-3" />
    </button>
    <button
      type="button"
      class="flex h-5 w-5 items-center justify-center rounded bg-base-100/80 text-base-content/70 transition-all hover:scale-105 hover:bg-base-100 hover:text-base-content active:scale-95"
      onclick={restoreMainWindow}
      aria-label="Restore main window"
      title="Restore main window"
    >
      <MonitorUp class="h-3 w-3" />
    </button>
    <button
      type="button"
      class="flex h-5 w-5 items-center justify-center rounded bg-base-100/80 text-base-content/70 transition-all hover:scale-105 hover:bg-error/15 hover:text-error active:scale-95"
      onclick={closeMiniPlayer}
      aria-label="Close mini player"
      title="Close mini player"
    >
      <X class="h-3 w-3" />
    </button>
  </div>

  <NowPlayingCenter
    mode="mini"
    {miniControlsVisible}
    currentTrack={playback.currentTrack}
    currentTime={playback.position}
    duration={playback.duration}
    {coverArtUrl}
    isPlaying={playback.isPlaying}
    canPlayPause={queue.items.length > 0 || !!queue.currentSong}
    canPrevious={queue.hasPrevious}
    canNext={queue.hasNext}
    onPlayPause={() => playback.togglePlayPause()}
    onPrevious={() => queue.playPrevious()}
    onNext={() => queue.playNext()}
    onSeek={handleSeek}
  />
</div>
