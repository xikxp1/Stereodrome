<script lang="ts">
  import { searchStore } from "$lib/stores/search.svelte";
  import {
    SkipBack,
    Play,
    Pause,
    SkipForward,
    Volume1,
    Volume2,
    Loader2,
  } from "lucide-svelte";

  interface Props {
    isPlaying?: boolean;
    currentTrack?: { title: string; artist: string; album: string } | null;
    currentTime?: number;
    duration?: number;
    volume?: number;
    onPlayPause?: () => void;
    onPrevious?: () => void;
    onNext?: () => void;
    onSeek?: (time: number) => void;
    onVolumeChange?: (volume: number) => void;
  }

  let {
    isPlaying = false,
    currentTrack = null,
    currentTime = 0,
    duration = 0,
    volume = 80,
    onPlayPause,
    onPrevious,
    onNext,
    onSeek,
    onVolumeChange,
  }: Props = $props();

  function handleInput(e: Event) {
    const input = e.target as HTMLInputElement;
    searchStore.setQuery(input.value);
  }

  function formatTime(seconds: number): string {
    if (!seconds || isNaN(seconds)) return "0:00";
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  }

  function formatTimeRemaining(current: number, total: number): string {
    const remaining = total - current;
    if (!remaining || isNaN(remaining)) return "-0:00";
    const mins = Math.floor(remaining / 60);
    const secs = Math.floor(remaining % 60);
    return `-${mins}:${secs.toString().padStart(2, "0")}`;
  }
</script>

<!-- Toolbar -->
<div
  class="relative flex h-20 items-center justify-between border-b border-base-300 bg-gradient-to-b from-base-200 to-base-300 px-3 select-none"
>
  <!-- Left: Playback Controls -->
  <div class="z-10 flex items-center gap-4">
    <!-- Button Group -->
    <div class="flex rounded bg-base-300 p-0.5 shadow-sm">
      <button
        class="flex h-7 w-7 items-center justify-center rounded-l bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content active:bg-base-300"
        onclick={() => onPrevious?.()}
        aria-label="Previous track"
      >
        <SkipBack class="h-3 w-3" fill="currentColor" />
      </button>
      <button
        class="flex h-7 w-8 items-center justify-center border-x border-base-300 bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content active:bg-base-300"
        onclick={() => onPlayPause?.()}
        aria-label={isPlaying ? "Pause" : "Play"}
      >
        {#if isPlaying}
          <Pause class="h-3.5 w-3.5" fill="currentColor" />
        {:else}
          <Play class="ml-0.5 h-3.5 w-3.5" fill="currentColor" />
        {/if}
      </button>
      <button
        class="flex h-7 w-7 items-center justify-center rounded-r bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content active:bg-base-300"
        onclick={() => onNext?.()}
        aria-label="Next track"
      >
        <SkipForward class="h-3 w-3" fill="currentColor" />
      </button>
    </div>

    <!-- Volume -->
    <div class="flex items-center gap-1.5">
      <Volume1 class="h-3 w-3 shrink-0 text-base-content/40" />
      <div class="relative flex h-3 w-16 items-center">
        <div class="absolute h-0.5 w-full rounded-full bg-base-300"></div>
        <div
          class="absolute h-0.5 rounded-full bg-base-content/30"
          style="width: {volume}%"
        ></div>
        <input
          type="range"
          min="0"
          max="100"
          value={volume}
          class="absolute w-full cursor-pointer opacity-0"
          oninput={(e) => onVolumeChange?.(Number(e.currentTarget.value))}
          aria-label="Volume"
        />
        <div
          class="pointer-events-none absolute h-2.5 w-2.5 rounded-full border border-base-300 bg-base-100 shadow-sm"
          style="left: calc({volume}% - 5px)"
        ></div>
      </div>
      <Volume2 class="h-3 w-3 shrink-0 text-base-content/40" />
    </div>
  </div>

  <!-- Center: Now Playing (absolutely positioned) -->
  <div
    class="pointer-events-none absolute left-1/2 top-1/2 flex w-full max-w-sm -translate-x-1/2 -translate-y-1/2 justify-center px-4"
  >
    <div
      class="pointer-events-auto w-full rounded border border-base-300 bg-base-100 px-3 py-1.5 shadow-sm"
    >
      <!-- Track Info (two lines) -->
      {#if currentTrack}
        <div class="mb-1 text-center">
          <div class="truncate text-sm font-medium text-base-content">
            {currentTrack.title}
          </div>
          <div class="truncate text-xs text-base-content/60">
            {currentTrack.artist}{#if currentTrack.album}&nbsp;— {currentTrack.album}{/if}
          </div>
        </div>
      {:else}
        <div class="mb-1 text-center text-xs text-base-content/40">
          Not Playing
        </div>
      {/if}

      <!-- Scrubber -->
      <div class="flex items-center gap-2">
        <span
          class="w-8 text-right font-mono text-[9px] tabular-nums text-base-content/50"
        >
          {formatTime(currentTime)}
        </span>
        <div class="relative flex h-3 flex-1 items-center">
          <div class="absolute h-1 w-full rounded-full bg-base-300"></div>
          <div
            class="absolute h-1 rounded-full bg-primary"
            style="width: {duration > 0 ? (currentTime / duration) * 100 : 0}%"
          ></div>
          <input
            type="range"
            min="0"
            max={duration || 100}
            value={currentTime}
            class="absolute w-full cursor-pointer opacity-0"
            oninput={(e) => onSeek?.(Number(e.currentTarget.value))}
            aria-label="Seek"
          />
          <div
            class="pointer-events-none absolute h-2.5 w-2.5 rounded-full border border-primary/50 bg-base-100 shadow-sm"
            style="left: calc({duration > 0
              ? (currentTime / duration) * 100
              : 0}% - 5px)"
          ></div>
        </div>
        <span
          class="w-8 font-mono text-[9px] tabular-nums text-base-content/50"
        >
          {formatTimeRemaining(currentTime, duration)}
        </span>
      </div>
    </div>
  </div>

  <!-- Right: Search -->
  <div class="z-10 relative flex items-center gap-2">
    <input
      type="search"
      placeholder="Search"
      value={searchStore.query}
      oninput={handleInput}
      class="h-6 w-40 rounded-full border border-base-300 bg-base-100 px-2.5 pr-6 text-[11px] outline-none transition-all duration-150 placeholder:text-base-content/40 focus:w-48 focus:border-primary focus:ring-2 focus:ring-primary/20"
    />
    {#if searchStore.isSearching}
      <span class="absolute right-2 top-1/2 -translate-y-1/2">
        <Loader2 class="h-3.5 w-3.5 animate-spin text-primary" />
      </span>
    {/if}
  </div>
</div>
