<script lang="ts">
  import { searchStore } from "$lib/stores/search.svelte";

  interface Props {
    isPlaying?: boolean;
    currentTrack?: { title: string; artist: string } | null;
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

  function handleClear() {
    searchStore.clear();
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
  class="relative flex h-16 items-center justify-between border-b border-base-300 bg-gradient-to-b from-base-200 to-base-300 px-3 select-none"
>
  <!-- Left: Playback Controls -->
  <div class="z-10 flex items-center gap-4">
    <!-- Button Group -->
    <div class="flex rounded-lg bg-base-300 p-0.5 shadow-sm">
      <button
        class="flex h-8 w-8 items-center justify-center rounded-l-md bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content active:bg-base-300"
        onclick={() => onPrevious?.()}
        aria-label="Previous track"
      >
        <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="currentColor">
          <path d="M6 6h2v12H6V6zm3.5 6l8.5 6V6l-8.5 6z" />
        </svg>
      </button>
      <button
        class="flex h-8 w-10 items-center justify-center border-x border-base-300 bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content active:bg-base-300"
        onclick={() => onPlayPause?.()}
        aria-label={isPlaying ? "Pause" : "Play"}
      >
        {#if isPlaying}
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor">
            <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z" />
          </svg>
        {:else}
          <svg class="ml-0.5 h-4 w-4" viewBox="0 0 24 24" fill="currentColor">
            <path d="M8 5v14l11-7L8 5z" />
          </svg>
        {/if}
      </button>
      <button
        class="flex h-8 w-8 items-center justify-center rounded-r-md bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content active:bg-base-300"
        onclick={() => onNext?.()}
        aria-label="Next track"
      >
        <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="currentColor">
          <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
        </svg>
      </button>
    </div>

    <!-- Volume -->
    <div class="flex items-center gap-2">
      <svg
        class="h-3.5 w-3.5 shrink-0 text-base-content/40"
        viewBox="0 0 24 24"
        fill="currentColor"
      >
        <path d="M3 9v6h4l5 5V4L7 9H3z" />
      </svg>
      <div class="relative flex h-4 w-20 items-center">
        <div class="absolute h-1 w-full rounded-full bg-base-300"></div>
        <div
          class="absolute h-1 rounded-full bg-base-content/30"
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
          class="pointer-events-none absolute h-3 w-3 rounded-full border border-base-300 bg-base-100 shadow-sm"
          style="left: calc({volume}% - 6px)"
        ></div>
      </div>
      <svg
        class="h-3.5 w-3.5 shrink-0 text-base-content/40"
        viewBox="0 0 24 24"
        fill="currentColor"
      >
        <path
          d="M3 9v6h4l5 5V4L7 9H3zm13.5 3A4.5 4.5 0 0014 7.97v8.05a4.49 4.49 0 002.5-3.02zM14 3.23v2.06a7.007 7.007 0 010 13.42v2.06A9.02 9.02 0 0023 12a9.02 9.02 0 00-9-8.77z"
        />
      </svg>
    </div>
  </div>

  <!-- Center: Now Playing (absolutely positioned) -->
  <div
    class="pointer-events-none absolute left-1/2 top-1/2 flex w-full max-w-sm -translate-x-1/2 -translate-y-1/2 justify-center px-4"
  >
    <div
      class="pointer-events-auto w-full rounded-lg border border-base-300 bg-base-100 px-4 py-2 shadow-sm"
    >
      <!-- Track Info -->
      <div class="mb-1.5 text-center">
        {#if currentTrack}
          <div class="truncate text-xs font-semibold text-base-content">
            {currentTrack.title}
          </div>
          <div class="truncate text-[10px] text-base-content/50">
            {currentTrack.artist}
          </div>
        {:else}
          <div class="text-xs text-base-content/40">Not Playing</div>
        {/if}
      </div>

      <!-- Scrubber -->
      <div class="flex items-center gap-2">
        <span
          class="w-8 text-right font-mono text-[9px] tabular-nums text-base-content/40"
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
          class="w-8 font-mono text-[9px] tabular-nums text-base-content/40"
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
      class="h-7 w-44 rounded-full border border-base-300 bg-base-100 px-3 pr-7 text-xs outline-none transition-all duration-150 placeholder:text-base-content/40 focus:w-52 focus:border-primary focus:ring-2 focus:ring-primary/20"
    />
    {#if searchStore.isSearching}
      <span class="absolute right-2 top-1/2 -translate-y-1/2">
        <svg
          class="h-3.5 w-3.5 animate-spin text-primary"
          viewBox="0 0 24 24"
          fill="none"
        >
          <circle
            class="opacity-25"
            cx="12"
            cy="12"
            r="10"
            stroke="currentColor"
            stroke-width="4"
          ></circle>
          <path
            class="opacity-75"
            fill="currentColor"
            d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
          ></path>
        </svg>
      </span>
    {:else if searchStore.hasQuery}
      <button
        class="absolute right-2 top-1/2 -translate-y-1/2 text-base-content/40 hover:text-base-content"
        onclick={handleClear}
        aria-label="Clear search"
      >
        <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="currentColor">
          <path
            d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"
          />
        </svg>
      </button>
    {/if}
    {#if searchStore.hasActiveQuery}
      <span class="text-[10px] text-primary font-medium"> Filtered </span>
    {/if}
  </div>
</div>
