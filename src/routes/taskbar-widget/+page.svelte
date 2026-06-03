<script lang="ts">
  import { getCoverArt } from "$lib/api/commands";
  import { playback } from "$lib/stores/playback.svelte";
  import { queue } from "$lib/stores/queue.svelte";
  import { error } from "@tauri-apps/plugin-log";
  import { Music, Pause, Play, SkipBack, SkipForward } from "lucide-svelte";

  const currentTrack = $derived(playback.currentTrack);
  const canPlayPause = $derived(queue.items.length > 0 || !!queue.currentSong);

  let coverArtUrl = $state<string | null>(null);
  let lastCoverArtId = $state<string | null>(null);

  $effect(() => {
    const coverArtId = currentTrack?.coverArtId ?? null;
    if (!coverArtId) {
      coverArtUrl = null;
      lastCoverArtId = null;
      return;
    }

    if (coverArtId === lastCoverArtId) return;
    lastCoverArtId = coverArtId;

    getCoverArt(coverArtId, 64)
      .then((url) => {
        if (lastCoverArtId === coverArtId) {
          coverArtUrl = url;
        }
      })
      .catch((e) => {
        error(`Failed to fetch taskbar widget cover art: ${e}`);
        if (lastCoverArtId === coverArtId) {
          coverArtUrl = null;
        }
      });
  });
</script>

<svelte:head>
  <title>Stereodrome Taskbar Widget</title>
</svelte:head>

<div class="taskbar-widget">
  <div class="cover" aria-hidden="true">
    {#if coverArtUrl}
      <img src={coverArtUrl} alt="" />
    {:else}
      <Music class="h-5 w-5 opacity-60" />
    {/if}
  </div>

  <div class="track-info">
    <div class="track-title">
      {currentTrack?.title || "Stereodrome"}
    </div>
    <div class="track-artist">
      {currentTrack?.artist || "Nothing playing"}
    </div>
  </div>

  <div class="controls">
    <button
      type="button"
      onclick={() => queue.playPrevious()}
      disabled={!queue.hasPrevious}
      aria-label="Previous track"
      title="Previous"
    >
      <SkipBack class="h-3.5 w-3.5" fill="currentColor" />
    </button>
    <button
      type="button"
      class="primary"
      onclick={() => playback.togglePlayPause()}
      disabled={!canPlayPause}
      aria-label={playback.isPlaying ? "Pause" : "Play"}
      title={playback.isPlaying ? "Pause" : "Play"}
    >
      {#if playback.isPlaying}
        <Pause class="h-3.5 w-3.5" fill="currentColor" />
      {:else}
        <Play class="ml-0.5 h-3.5 w-3.5" fill="currentColor" />
      {/if}
    </button>
    <button
      type="button"
      onclick={() => queue.playNext()}
      disabled={!queue.hasNext}
      aria-label="Next track"
      title="Next"
    >
      <SkipForward class="h-3.5 w-3.5" fill="currentColor" />
    </button>
  </div>
</div>

<style>
  :global(body) {
    overflow: hidden;
    background: transparent;
  }

  .taskbar-widget {
    display: grid;
    grid-template-columns: 34px minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    width: 100vw;
    height: 100vh;
    padding: 2px 8px 2px 4px;
    overflow: hidden;
    color: oklch(95% 0.01 250);
    font-family:
      "Segoe UI Variable",
      "Segoe UI",
      system-ui,
      -apple-system,
      sans-serif;
    user-select: none;
  }

  .cover {
    display: flex;
    width: 34px;
    height: 34px;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    border-radius: 5px;
    background: oklch(25% 0.02 250 / 0.72);
    color: oklch(80% 0.04 250);
  }

  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .track-info {
    min-width: 0;
    line-height: 1.12;
  }

  .track-title,
  .track-artist {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-title {
    font-size: 12px;
    font-weight: 600;
  }

  .track-artist {
    margin-top: 2px;
    font-size: 11px;
    color: oklch(86% 0.02 250 / 0.68);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 1px;
  }

  button {
    display: flex;
    width: 25px;
    height: 25px;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: currentColor;
    transition:
      background 120ms ease,
      opacity 120ms ease;
  }

  button:hover:not(:disabled) {
    background: oklch(95% 0.01 250 / 0.1);
  }

  button:active:not(:disabled) {
    background: oklch(95% 0.01 250 / 0.16);
  }

  button:disabled {
    opacity: 0.34;
  }

  button.primary {
    background: oklch(95% 0.01 250 / 0.08);
  }
</style>
