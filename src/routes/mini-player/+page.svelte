<script lang="ts">
  import NowPlayingCenter from "$lib/components/NowPlayingCenter.svelte";
  import {
    closeMiniPlayer as closeMiniPlayerCommand,
    getCoverArt,
    restoreMainWindow as restoreMainWindowCommand,
    setMiniPlayerMode,
    setMiniPlayerPosition,
    seekPlayback,
  } from "$lib/api/commands";
  import { playback } from "$lib/stores/playback.svelte";
  import { queue } from "$lib/stores/queue.svelte";
  import { listen } from "@tauri-apps/api/event";
  import {
    availableMonitors,
    currentMonitor,
    getCurrentWindow,
    type Monitor,
  } from "@tauri-apps/api/window";
  import { error } from "@tauri-apps/plugin-log";
  import { onMount } from "svelte";
  import {
    AudioLines,
    GripHorizontal,
    Minimize2,
    MonitorUp,
    X,
  } from "lucide-svelte";
  import type { MiniPlayerHoverState, MiniPlayerPosition } from "$lib/types";

  const POSITION_PERSIST_DEBOUNCE_MS = 250;
  const MINI_PLAYER_WIDTH = 320;
  const MINI_PLAYER_HEIGHT = 72;
  const NANO_PLAYER_SIZE = 30;
  const NANO_PLAYER_MARGIN = 0;

  interface LogicalMonitorBounds {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  const currentTrack = $derived(playback.currentTrack);

  let coverArtUrl = $state<string | null>(null);
  let lastCoverArtId = $state<string | null>(null);
  let closing = $state(false);
  let isNanoMode = $state(false);
  let isHovered = $state(false);
  let persistedMiniPlayerX = $state<number | null>(null);
  let persistedMiniPlayerY = $state<number | null>(null);
  let preNanoMiniPlayerX = $state<number | null>(null);
  let preNanoMiniPlayerY = $state<number | null>(null);
  let persistPositionTimeout = $state<ReturnType<typeof setTimeout> | null>(
    null
  );

  // Reveal controls immediately on hover; force-hide them when window blurs.
  const miniControlsVisible = $derived(isHovered && !isNanoMode);
  const miniPreviousTooltip = $derived.by(() => {
    const song = queue.previousSong;
    if (!song) {
      return "Previous";
    }
    return `${song.artist || "Unknown Artist"} - ${song.title || "Unknown Title"}`;
  });
  const miniNextTooltip = $derived.by(() => {
    const song = queue.nextSong;
    if (!song) {
      return "Next";
    }
    return `${song.artist || "Unknown Artist"} - ${song.title || "Unknown Title"}`;
  });
  const nanoPlayerTooltip = $derived.by(() => {
    const currentLine = currentTrack
      ? `${currentTrack.artist || "Unknown Artist"} - ${currentTrack.title || "Unknown Title"}`
      : null;
    const nextLine = queue.nextSong
      ? `${queue.nextSong.artist || "Unknown Artist"} - ${queue.nextSong.title || "Unknown Title"}`
      : null;

    const lines = [currentLine, nextLine].filter(
      (line): line is string => line !== null
    );

    if (lines.length === 0) {
      return "Unknown Artist - Unknown Title";
    }

    return lines.join("\n");
  });

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

  function latestPersistedMiniPlayerPosition(): MiniPlayerPosition | null {
    if (persistedMiniPlayerX === null || persistedMiniPlayerY === null) {
      return null;
    }

    // Return a plain object so invoke payload is never a reactive proxy.
    return {
      x: persistedMiniPlayerX,
      y: persistedMiniPlayerY,
    };
  }

  function preNanoMiniPlayerPosition(): MiniPlayerPosition | null {
    if (preNanoMiniPlayerX === null || preNanoMiniPlayerY === null) {
      return null;
    }
    return {
      x: preNanoMiniPlayerX,
      y: preNanoMiniPlayerY,
    };
  }

  function setPreNanoMiniPlayerPosition(position: MiniPlayerPosition): void {
    preNanoMiniPlayerX = position.x;
    preNanoMiniPlayerY = position.y;
  }

  async function persistMiniPlayerPosition(
    position: MiniPlayerPosition
  ): Promise<void> {
    await setMiniPlayerPosition({
      x: position.x,
      y: position.y,
    });
  }

  function schedulePositionPersistence(position: MiniPlayerPosition) {
    if (isNanoMode) return;

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

  function getLogicalMonitorBounds(monitor: Monitor): LogicalMonitorBounds {
    const scale = monitor.scaleFactor || 1;
    return {
      x: monitor.workArea.position.x / scale,
      y: monitor.workArea.position.y / scale,
      width: monitor.workArea.size.width / scale,
      height: monitor.workArea.size.height / scale,
    };
  }

  function monitorContainsPosition(
    bounds: LogicalMonitorBounds,
    position: MiniPlayerPosition
  ): boolean {
    return (
      position.x >= bounds.x &&
      position.y >= bounds.y &&
      position.x < bounds.x + bounds.width &&
      position.y < bounds.y + bounds.height
    );
  }

  function clampNanoPositionToMonitor(
    position: MiniPlayerPosition,
    bounds: LogicalMonitorBounds
  ): MiniPlayerPosition {
    const maxX = bounds.x + Math.max(0, bounds.width - NANO_PLAYER_SIZE);
    const maxY = bounds.y + Math.max(0, bounds.height - NANO_PLAYER_SIZE);
    return {
      x: Math.round(Math.min(Math.max(position.x, bounds.x), maxX)),
      y: Math.round(Math.min(Math.max(position.y, bounds.y), maxY)),
    };
  }

  async function resolveMonitorBoundsForPosition(
    position: MiniPlayerPosition
  ): Promise<LogicalMonitorBounds | null> {
    const [monitors, activeMonitor] = await Promise.all([
      availableMonitors(),
      currentMonitor(),
    ]);

    const monitorBounds = monitors.map(getLogicalMonitorBounds);
    const matchedBounds = monitorBounds.find((bounds) =>
      monitorContainsPosition(bounds, position)
    );
    if (matchedBounds) {
      return matchedBounds;
    }

    if (activeMonitor) {
      return getLogicalMonitorBounds(activeMonitor);
    }

    return monitorBounds[0] ?? null;
  }

  function resolveNanoPosition(
    miniPosition: MiniPlayerPosition,
    bounds: LogicalMonitorBounds
  ): MiniPlayerPosition {
    const leftDistance = miniPosition.x - bounds.x;
    const rightDistance =
      bounds.x + bounds.width - (miniPosition.x + MINI_PLAYER_WIDTH);
    const topDistance = miniPosition.y - bounds.y;
    const bottomDistance =
      bounds.y + bounds.height - (miniPosition.y + MINI_PLAYER_HEIGHT);

    const useLeft = leftDistance <= rightDistance;
    const useTop = topDistance <= bottomDistance;

    const leftX = bounds.x + NANO_PLAYER_MARGIN;
    const rightX =
      bounds.x +
      Math.max(0, bounds.width - NANO_PLAYER_SIZE - NANO_PLAYER_MARGIN);
    const topY = bounds.y + NANO_PLAYER_MARGIN;
    const bottomY =
      bounds.y +
      Math.max(0, bounds.height - NANO_PLAYER_SIZE - NANO_PLAYER_MARGIN);

    return clampNanoPositionToMonitor(
      {
        x: useLeft ? leftX : rightX,
        y: useTop ? topY : bottomY,
      },
      bounds
    );
  }

  async function currentWindowPosition(): Promise<MiniPlayerPosition> {
    const currentWindow = getCurrentWindow();
    const [scale, outerPosition] = await Promise.all([
      currentWindow.scaleFactor(),
      currentWindow.outerPosition(),
    ]);
    return {
      x: outerPosition.x / scale,
      y: outerPosition.y / scale,
    };
  }

  async function persistCurrentWindowPosition(): Promise<void> {
    await persistMiniPlayerPosition(await currentWindowPosition());
  }

  async function persistPositionForCloseOrUnmount(): Promise<void> {
    if (isNanoMode) {
      const savedMiniPosition = preNanoMiniPlayerPosition();
      if (savedMiniPosition) {
        await persistMiniPlayerPosition(savedMiniPosition);
        return;
      }
    }
    await persistCurrentWindowPosition();
  }

  async function captureAndScheduleCurrentWindowPosition(): Promise<void> {
    schedulePositionPersistence(await currentWindowPosition());
  }

  async function minimizeToNanoPlayer() {
    if (isNanoMode) return;

    try {
      const miniPosition = await currentWindowPosition();
      const monitorBounds = await resolveMonitorBoundsForPosition(miniPosition);
      if (!monitorBounds) {
        error("Failed to minimize to nano player: no monitor bounds available");
        return;
      }

      const nanoPosition = resolveNanoPosition(miniPosition, monitorBounds);
      setPreNanoMiniPlayerPosition(miniPosition);
      isNanoMode = true;
      isHovered = false;
      await setMiniPlayerMode("nano", nanoPosition);
    } catch (e) {
      isNanoMode = false;
      error(`Failed to switch mini player to nano mode: ${e}`);
    }
  }

  async function restoreFromNanoPlayer() {
    if (!isNanoMode) return;

    try {
      const restorePosition =
        preNanoMiniPlayerPosition() ?? (await currentWindowPosition());
      await setMiniPlayerMode("mini", restorePosition);
      isNanoMode = false;
      schedulePositionPersistence(restorePosition);
    } catch (e) {
      error(`Failed to restore mini player from nano mode: ${e}`);
    }
  }

  async function closeMiniPlayer() {
    if (closing) return;
    closing = true;
    let closedSuccessfully = false;

    try {
      if (persistPositionTimeout) {
        clearTimeout(persistPositionTimeout);
        persistPositionTimeout = null;
      }
      await persistPositionForCloseOrUnmount();
      await closeMiniPlayerCommand();
      closedSuccessfully = true;
    } catch (e) {
      error(`Failed to close mini player window: ${e}`);
    } finally {
      if (closedSuccessfully) {
        isNanoMode = false;
        isHovered = false;
      }

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
        if (isNanoMode) {
          return;
        }

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
      }

      if (isNanoMode) {
        const savedMiniPosition = preNanoMiniPlayerPosition();
        if (savedMiniPosition) {
          void persistMiniPlayerPosition(savedMiniPosition).catch((e) => {
            error(`Failed to persist mini player position: ${e}`);
          });
        }
        return;
      }

      const latestPosition = latestPersistedMiniPlayerPosition();
      if (latestPosition) {
        void persistMiniPlayerPosition(latestPosition).catch((e) => {
          error(`Failed to persist mini player position: ${e}`);
        });
      }
    };
  });
</script>

<svelte:head>
  <title>Stereodrome Mini Player</title>
</svelte:head>

<div
  class={`group/mini relative flex h-screen w-screen overflow-hidden ${
    isNanoMode
      ? "bg-transparent p-0"
      : "items-center bg-linear-to-b from-base-200 to-base-300 px-0.5 py-0 shadow-lg"
  }`}
  role="group"
  onpointerenter={() => {
    isHovered = true;
  }}
  onpointerleave={() => {
    isHovered = false;
  }}
>
  {#if isNanoMode}
    <button
      type="button"
      class="relative flex h-full w-full items-center justify-center overflow-hidden border border-base-content/30 bg-transparent text-base-content/90 transition-colors"
      onclick={restoreFromNanoPlayer}
      aria-label="Restore mini player"
      title={nanoPlayerTooltip}
    >
      <span
        class="pointer-events-none absolute inset-0 bg-base-100 opacity-25 transition-opacity hover:opacity-35 active:opacity-45"
      ></span>
      <AudioLines class="relative z-10 h-4 w-4" />
    </button>
  {:else}
    <div
      class={`absolute left-[4.25rem] top-1 z-30 flex items-center gap-1 transition-opacity ${
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
    </div>

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
        onclick={minimizeToNanoPlayer}
        aria-label="Minimize to nano player"
        title="Minimize to nano player"
      >
        <Minimize2 class="h-3 w-3" />
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
      previousTrackTooltip={miniPreviousTooltip}
      nextTrackTooltip={miniNextTooltip}
      onPlayPause={() => playback.togglePlayPause()}
      onPrevious={() => queue.playPrevious()}
      onNext={() => queue.playNext()}
      onSeek={handleSeek}
    />
  {/if}
</div>
