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
    onSearchResultClick?: (type: string, id: string) => void;
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
    onSearchResultClick,
  }: Props = $props();

  function handleSearch(e: Event) {
    const input = e.target as HTMLInputElement;
    searchStore.search(input.value);
  }

  function handleResultClick(type: string, id: string) {
    searchStore.close();
    onSearchResultClick?.(type, id);
  }

  function formatDuration(seconds: number | null): string {
    if (!seconds) return "";
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
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
      class="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-base-content/40 z-10"
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
      value={searchStore.query}
      oninput={handleSearch}
      onfocus={() => searchStore.open()}
      class="search-field w-full pl-7 pr-3 py-1 text-xs text-base-content placeholder-base-content/40"
    />

    <!-- Search Results Dropdown -->
    {#if searchStore.isOpen && searchStore.hasResults}
      <div
        class="absolute top-full right-0 mt-1 w-72 bg-base-100 border border-base-300 rounded-lg shadow-xl z-50 max-h-80 overflow-y-auto"
      >
        {#if searchStore.results.songs.length > 0}
          <div
            class="px-2 py-1 text-[10px] font-semibold text-base-content/50 uppercase bg-base-200"
          >
            Songs
          </div>
          {#each searchStore.results.songs.slice(0, 5) as song (song.id)}
            <button
              class="w-full px-2 py-1.5 text-left hover:bg-base-200 flex items-center gap-2"
              onclick={() => handleResultClick("song", song.id)}
            >
              <svg
                class="w-3 h-3 text-base-content/40 flex-shrink-0"
                viewBox="0 0 24 24"
                fill="currentColor"
              >
                <path
                  d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
                />
              </svg>
              <div class="flex-1 min-w-0">
                <div class="text-xs font-medium truncate">{song.title}</div>
                <div class="text-[10px] text-base-content/50 truncate">
                  {song.artist || "Unknown"} — {song.album || "Unknown"}
                </div>
              </div>
              {#if song.duration}
                <span class="text-[10px] text-base-content/40 tabular-nums"
                  >{formatDuration(song.duration)}</span
                >
              {/if}
            </button>
          {/each}
        {/if}

        {#if searchStore.results.albums.length > 0}
          <div
            class="px-2 py-1 text-[10px] font-semibold text-base-content/50 uppercase bg-base-200"
          >
            Albums
          </div>
          {#each searchStore.results.albums.slice(0, 3) as album (album.id)}
            <button
              class="w-full px-2 py-1.5 text-left hover:bg-base-200 flex items-center gap-2"
              onclick={() => handleResultClick("album", album.id)}
            >
              <svg
                class="w-3 h-3 text-base-content/40 flex-shrink-0"
                viewBox="0 0 24 24"
                fill="currentColor"
              >
                <path
                  d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z"
                />
              </svg>
              <div class="flex-1 min-w-0">
                <div class="text-xs font-medium truncate">{album.name}</div>
                <div class="text-[10px] text-base-content/50 truncate">
                  {album.artist || "Unknown"}{album.year
                    ? ` (${album.year})`
                    : ""}
                </div>
              </div>
              <span class="text-[10px] text-base-content/40"
                >{album.song_count} songs</span
              >
            </button>
          {/each}
        {/if}

        {#if searchStore.results.artists.length > 0}
          <div
            class="px-2 py-1 text-[10px] font-semibold text-base-content/50 uppercase bg-base-200"
          >
            Artists
          </div>
          {#each searchStore.results.artists.slice(0, 3) as artist (artist.id)}
            <button
              class="w-full px-2 py-1.5 text-left hover:bg-base-200 flex items-center gap-2"
              onclick={() => handleResultClick("artist", artist.id)}
            >
              <svg
                class="w-3 h-3 text-base-content/40 flex-shrink-0"
                viewBox="0 0 24 24"
                fill="currentColor"
              >
                <path
                  d="M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z"
                />
              </svg>
              <div class="flex-1 min-w-0">
                <div class="text-xs font-medium truncate">{artist.name}</div>
              </div>
              <span class="text-[10px] text-base-content/40"
                >{artist.album_count} albums</span
              >
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </div>
</div>
