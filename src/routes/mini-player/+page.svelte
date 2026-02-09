<script lang="ts">
  import NowPlayingCenter from "$lib/components/NowPlayingCenter.svelte";
  import {
    closeMiniPlayer as closeMiniPlayerCommand,
    getCoverArt,
    restoreMainWindow as restoreMainWindowCommand,
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

  const MINI_PLAYER_POSITION_KEY = "mini_player_position_v1";

  const currentTrack = $derived(playback.currentTrack);

  let coverArtUrl = $state<string | null>(null);
  let lastCoverArtId = $state<string | null>(null);
  let closing = $state(false);
  let isHovered = $state(false);

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

  async function closeMiniPlayer() {
    if (closing) return;
    closing = true;

    try {
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
          const x = event.payload.x / scale;
          const y = event.payload.y / scale;
          localStorage.setItem(
            MINI_PLAYER_POSITION_KEY,
            JSON.stringify({ x, y })
          );
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
