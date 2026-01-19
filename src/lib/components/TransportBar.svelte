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
  <div class="dropdown dropdown-end z-10">
    <input
      type="search"
      placeholder="Search"
      value={searchStore.query}
      oninput={handleSearch}
      onfocus={() => searchStore.open()}
      class="h-7 w-36 rounded-full border border-base-300 bg-base-100 px-3 text-xs outline-none transition-all duration-150 placeholder:text-base-content/40 focus:w-44 focus:border-primary focus:ring-2 focus:ring-primary/20"
    />

    {#if searchStore.isOpen && searchStore.hasResults}
      <ul
        class="menu dropdown-content z-50 mt-1 max-h-80 w-72 overflow-y-auto rounded-lg border border-base-300 bg-base-100 p-0 shadow-lg"
      >
        {#if searchStore.results.songs.length > 0}
          <li
            class="border-b border-base-200 bg-base-200/50 px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wide text-base-content/50"
          >
            Songs
          </li>
          {#each searchStore.results.songs.slice(0, 5) as song (song.id)}
            <li>
              <button
                class="flex items-center gap-2 rounded-none px-3 py-2 hover:bg-primary/10"
                onclick={() => handleResultClick("song", song.id)}
              >
                <svg
                  class="h-3.5 w-3.5 shrink-0 text-base-content/40"
                  viewBox="0 0 24 24"
                  fill="currentColor"
                >
                  <path
                    d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
                  />
                </svg>
                <div class="min-w-0 flex-1">
                  <div class="truncate text-xs font-medium">{song.title}</div>
                  <div class="truncate text-[10px] text-base-content/50">
                    {song.artist || "Unknown"} — {song.album || "Unknown"}
                  </div>
                </div>
                {#if song.duration}
                  <span class="shrink-0 text-[10px] text-base-content/40">
                    {formatDuration(song.duration)}
                  </span>
                {/if}
              </button>
            </li>
          {/each}
        {/if}

        {#if searchStore.results.albums.length > 0}
          <li
            class="border-b border-base-200 bg-base-200/50 px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wide text-base-content/50"
          >
            Albums
          </li>
          {#each searchStore.results.albums.slice(0, 3) as album (album.id)}
            <li>
              <button
                class="flex items-center gap-2 rounded-none px-3 py-2 hover:bg-primary/10"
                onclick={() => handleResultClick("album", album.id)}
              >
                <svg
                  class="h-3.5 w-3.5 shrink-0 text-base-content/40"
                  viewBox="0 0 24 24"
                  fill="currentColor"
                >
                  <path
                    d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z"
                  />
                </svg>
                <div class="min-w-0 flex-1">
                  <div class="truncate text-xs font-medium">{album.name}</div>
                  <div class="truncate text-[10px] text-base-content/50">
                    {album.artist || "Unknown"}{album.year
                      ? ` (${album.year})`
                      : ""}
                  </div>
                </div>
                <span class="shrink-0 text-[10px] text-base-content/40">
                  {album.song_count} songs
                </span>
              </button>
            </li>
          {/each}
        {/if}

        {#if searchStore.results.artists.length > 0}
          <li
            class="border-b border-base-200 bg-base-200/50 px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wide text-base-content/50"
          >
            Artists
          </li>
          {#each searchStore.results.artists.slice(0, 3) as artist (artist.id)}
            <li>
              <button
                class="flex items-center gap-2 rounded-none px-3 py-2 hover:bg-primary/10"
                onclick={() => handleResultClick("artist", artist.id)}
              >
                <svg
                  class="h-3.5 w-3.5 shrink-0 text-base-content/40"
                  viewBox="0 0 24 24"
                  fill="currentColor"
                >
                  <path
                    d="M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z"
                  />
                </svg>
                <div class="min-w-0 flex-1">
                  <div class="truncate text-xs font-medium">{artist.name}</div>
                </div>
                <span class="shrink-0 text-[10px] text-base-content/40">
                  {artist.album_count} albums
                </span>
              </button>
            </li>
          {/each}
        {/if}
      </ul>
    {/if}
  </div>
</div>
