<script lang="ts">
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
    onSearch?: (query: string) => void;
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
    onSearch,
  }: Props = $props();

  let searchQuery = $state("");

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

  const progress = $derived(duration > 0 ? (currentTime / duration) * 100 : 0);
</script>

<div class="transport-bar h-[72px] flex items-center px-3 gap-3 select-none">
  <!-- Playback Controls -->
  <div class="flex items-center gap-1">
    <!-- Previous -->
    <button
      class="transport-btn"
      onclick={() => onPrevious?.()}
      aria-label="Previous track"
    >
      <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="currentColor">
        <path d="M6 6h2v12H6V6zm3.5 6l8.5 6V6l-8.5 6z" />
      </svg>
    </button>

    <!-- Play/Pause -->
    <button
      class="transport-btn-play"
      onclick={() => onPlayPause?.()}
      aria-label={isPlaying ? "Pause" : "Play"}
    >
      {#if isPlaying}
        <svg class="w-4 h-4" viewBox="0 0 24 24" fill="currentColor">
          <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z" />
        </svg>
      {:else}
        <svg class="w-4 h-4 ml-0.5" viewBox="0 0 24 24" fill="currentColor">
          <path d="M8 5v14l11-7L8 5z" />
        </svg>
      {/if}
    </button>

    <!-- Next -->
    <button
      class="transport-btn"
      onclick={() => onNext?.()}
      aria-label="Next track"
    >
      <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="currentColor">
        <path d="M6 18l8.5-6L6 6v12zm2 0V6l6.5 6L8 18zM16 6v12h2V6h-2z" />
      </svg>
    </button>
  </div>

  <!-- Volume -->
  <div class="flex items-center gap-2 w-24">
    <svg
      class="w-3 h-3 text-neutral-content/60 flex-shrink-0"
      viewBox="0 0 24 24"
      fill="currentColor"
    >
      <path
        d="M3 9v6h4l5 5V4L7 9H3zm13.5 3A4.5 4.5 0 0014 7.97v8.05a4.49 4.49 0 002.5-3.02z"
      />
    </svg>
    <div class="relative flex-1 h-3 flex items-center">
      <div class="volume-track w-full">
        <div class="volume-fill" style="width: {volume}%"></div>
      </div>
      <input
        type="range"
        min="0"
        max="100"
        value={volume}
        class="absolute inset-0 w-full opacity-0 cursor-pointer"
        oninput={(e) => onVolumeChange?.(Number(e.currentTarget.value))}
        aria-label="Volume"
      />
    </div>
    <svg
      class="w-3.5 h-3.5 text-neutral-content/60 flex-shrink-0"
      viewBox="0 0 24 24"
      fill="currentColor"
    >
      <path
        d="M3 9v6h4l5 5V4L7 9H3zm13.5 3A4.5 4.5 0 0014 7.97v8.05a4.49 4.49 0 002.5-3.02zM14 3.23v2.06a7.007 7.007 0 010 13.42v2.06A9.02 9.02 0 0023 12a9.02 9.02 0 00-9-8.77z"
      />
    </svg>
  </div>

  <!-- LCD Display & Scrubber -->
  <div class="flex-1 flex items-center justify-center">
    <div class="lcd-display w-full max-w-md px-4 py-2">
      <!-- Track Info -->
      <div class="text-center mb-1.5">
        {#if currentTrack}
          <div class="text-xs font-medium text-neutral-content truncate">
            {currentTrack.title}
          </div>
          <div class="text-[10px] text-neutral-content/60 truncate">
            {currentTrack.artist}
          </div>
        {:else}
          <div class="text-xs text-neutral-content/40">Not Playing</div>
        {/if}
      </div>

      <!-- Scrubber -->
      <div class="flex items-center gap-2">
        <span
          class="text-[10px] text-neutral-content/60 w-8 text-right tabular-nums"
        >
          {formatTime(currentTime)}
        </span>
        <div class="relative flex-1 h-4 flex items-center">
          <div class="scrubber-track w-full">
            <div class="scrubber-fill" style="width: {progress}%"></div>
          </div>
          <input
            type="range"
            min="0"
            max={duration || 100}
            value={currentTime}
            class="absolute inset-0 w-full opacity-0 cursor-pointer"
            oninput={(e) => onSeek?.(Number(e.currentTarget.value))}
            aria-label="Seek"
          />
        </div>
        <span class="text-[10px] text-neutral-content/60 w-8 tabular-nums">
          {formatTimeRemaining(currentTime, duration)}
        </span>
      </div>
    </div>
  </div>

  <!-- View Controls (placeholder) -->
  <div class="flex items-center gap-0.5">
    <button class="transport-btn" aria-label="List view">
      <svg class="w-3 h-3" viewBox="0 0 24 24" fill="currentColor">
        <path d="M3 4h18v2H3V4zm0 7h18v2H3v-2zm0 7h18v2H3v-2z" />
      </svg>
    </button>
    <button class="transport-btn" aria-label="Grid view">
      <svg class="w-3 h-3" viewBox="0 0 24 24" fill="currentColor">
        <path
          d="M4 4h4v4H4V4zm6 0h4v4h-4V4zm6 0h4v4h-4V4zM4 10h4v4H4v-4zm6 0h4v4h-4v-4zm6 0h4v4h-4v-4zM4 16h4v4H4v-4zm6 0h4v4h-4v-4zm6 0h4v4h-4v-4z"
        />
      </svg>
    </button>
    <button class="transport-btn" aria-label="Cover Flow view">
      <svg class="w-3 h-3" viewBox="0 0 24 24" fill="currentColor">
        <path d="M2 6h4v12H2V6zm6 2h4v8H8V8zm6-2h4v12h-4V6zm6 2h4v8h-4V8z" />
      </svg>
    </button>
  </div>

  <!-- Search -->
  <div class="relative w-40">
    <svg
      class="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-base-content/40"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2.5"
    >
      <circle cx="11" cy="11" r="7" />
      <path d="m21 21-4.35-4.35" />
    </svg>
    <input
      type="search"
      placeholder="Search"
      bind:value={searchQuery}
      oninput={() => onSearch?.(searchQuery)}
      class="search-field w-full pl-7 pr-3 py-1 text-xs text-base-content placeholder-base-content/40"
    />
  </div>
</div>
