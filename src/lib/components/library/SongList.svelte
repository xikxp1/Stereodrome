<script lang="ts">
  import type { Song } from "$lib/types";
  import { createVirtualizer } from "@tanstack/svelte-virtual";
  import { SvelteSet } from "svelte/reactivity";
  import { AlertCircle, Music, Volume2 } from "lucide-svelte";

  interface Props {
    songs?: Song[];
    isLoading?: boolean;
    error?: Error | null;
    selectedSongId?: string | null;
    playingSongId?: string | null;
    onSelect?: (song: Song) => void;
    onPlay?: (song: Song) => void;
  }

  let {
    songs = [],
    isLoading = false,
    error = null,
    selectedSongId = null,
    playingSongId = null,
    onSelect,
    onPlay,
  }: Props = $props();

  let checkedSongs = new SvelteSet<string>();
  let scrollContainer: HTMLDivElement | null = $state(null);

  const ROW_HEIGHT = 28;

  // Create virtualizer - key is the ternary in getScrollElement
  const virtualizer = $derived(
    createVirtualizer<HTMLDivElement, HTMLDivElement>({
      count: songs.length,
      getScrollElement: scrollContainer ? () => scrollContainer : () => null,
      estimateSize: () => ROW_HEIGHT,
      overscan: 10,
    })
  );

  const virtualItems = $derived($virtualizer.getVirtualItems());
  const totalSize = $derived($virtualizer.getTotalSize());

  function formatDuration(seconds: number | null): string {
    if (!seconds) return "--:--";
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  }

  function handleRowClick(song: Song) {
    onSelect?.(song);
  }

  function handleRowDoubleClick(song: Song) {
    onPlay?.(song);
  }

  function toggleCheck(songId: string, e: Event) {
    e.stopPropagation();
    if (checkedSongs.has(songId)) {
      checkedSongs.delete(songId);
    } else {
      checkedSongs.add(songId);
    }
  }

  function toggleAllChecked() {
    if (checkedSongs.size === songs.length) {
      checkedSongs.clear();
    } else {
      songs.forEach((s) => checkedSongs.add(s.id));
    }
  }

  const allChecked = $derived(
    songs.length > 0 && checkedSongs.size === songs.length
  );
  const someChecked = $derived(
    checkedSongs.size > 0 && checkedSongs.size < songs.length
  );
</script>

<div class="song-list-container flex flex-col h-full bg-white select-none">
  {#if isLoading}
    <div class="flex-1 flex items-center justify-center">
      <div class="loading-dots">
        <span></span>
        <span></span>
        <span></span>
      </div>
    </div>
  {:else if error}
    <div class="empty-state">
      <AlertCircle class="empty-state-icon" />
      <div class="empty-state-title">Failed to load songs</div>
      <div class="empty-state-text">{error.message}</div>
    </div>
  {:else if songs.length === 0}
    <div class="empty-state">
      <Music class="empty-state-icon" />
      <div class="empty-state-title">No songs</div>
      <div class="empty-state-text">
        Select an artist or album to view songs
      </div>
    </div>
  {:else}
    <div class="flex-1 overflow-hidden flex flex-col">
      <!-- Fixed header -->
      <div class="song-grid-header">
        <div class="cell-checkbox">
          <input
            type="checkbox"
            class="itunes-checkbox"
            checked={allChecked}
            indeterminate={someChecked}
            onchange={toggleAllChecked}
          />
        </div>
        <div class="cell-track">#</div>
        <div class="cell-name">Name</div>
        <div class="cell-time">Time</div>
        <div class="cell-artist">Artist</div>
        <div class="cell-album">Album</div>
        <div class="cell-year">Year</div>
        <div class="cell-genre">Genre</div>
      </div>

      <!-- Virtualized song list body -->
      <div bind:this={scrollContainer} class="flex-1 overflow-auto">
        <div style="height: {totalSize}px; width: 100%; position: relative;">
          {#each virtualItems as row (row.index)}
            {@const song = songs[row.index]}
            {@const index = row.index}
            <div
              class="song-grid-row"
              class:selected={selectedSongId === song.id}
              class:playing={playingSongId === song.id}
              class:even={index % 2 === 1}
              onclick={() => handleRowClick(song)}
              ondblclick={() => handleRowDoubleClick(song)}
              style="position: absolute; top: 0; left: 0; width: 100%; height: {row.size}px; transform: translateY({row.start}px);"
            >
              <div class="cell-checkbox">
                <input
                  type="checkbox"
                  class="itunes-checkbox"
                  checked={checkedSongs.has(song.id)}
                  onchange={(e) => toggleCheck(song.id, e)}
                />
              </div>
              <div class="cell-track dimmed">
                {song.track_number || index + 1}
              </div>
              <div class="cell-name font-medium">
                {#if playingSongId === song.id}
                  <span class="inline-flex items-center gap-1.5">
                    <Volume2 class="w-3 h-3 animate-pulse" />
                    <span class="truncate">{song.title}</span>
                  </span>
                {:else}
                  <span class="truncate">{song.title}</span>
                {/if}
              </div>
              <div class="cell-time dimmed tabular-nums">
                {formatDuration(song.duration)}
              </div>
              <div class="cell-artist dimmed">
                <span class="truncate">{song.artist || "—"}</span>
              </div>
              <div class="cell-album dimmed">
                <span class="truncate">{song.album || "—"}</span>
              </div>
              <div class="cell-year dimmed">{song.year || "—"}</div>
              <div class="cell-genre dimmed">
                <span class="truncate">{song.genre || "—"}</span>
              </div>
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .song-list-container {
    container-type: inline-size;
  }

  .song-grid-header,
  .song-grid-row {
    display: grid;
    grid-template-columns: 32px 40px minmax(120px, 1fr) 52px minmax(100px, 0.6fr) minmax(100px, 0.8fr) 56px 96px;
    align-items: center;
    font-size: 0.75rem;
  }

  /* Hide genre column below 900px */
  @container (max-width: 900px) {
    .song-grid-header,
    .song-grid-row {
      grid-template-columns: 32px 40px minmax(120px, 1fr) 52px minmax(100px, 0.6fr) minmax(100px, 0.8fr) 56px;
    }

    .cell-genre {
      display: none;
    }
  }

  /* Hide year and genre columns below 800px */
  @container (max-width: 800px) {
    .song-grid-header,
    .song-grid-row {
      grid-template-columns: 32px 40px minmax(120px, 1fr) 52px minmax(80px, 0.6fr) minmax(80px, 0.8fr);
    }

    .cell-year,
    .cell-genre {
      display: none;
    }
  }

  .song-grid-header {
    background: linear-gradient(
      to bottom,
      oklch(97% 0.003 250) 0%,
      oklch(91% 0.005 250) 100%
    );
    border-bottom: 1px solid oklch(82% 0.008 250);
    font-size: 0.6875rem;
    font-weight: 600;
    color: oklch(42% 0.01 250);
    text-shadow: 0 1px 0 white;
    flex-shrink: 0;
  }

  .song-grid-header > div {
    padding: 0.375rem 0.75rem;
    white-space: nowrap;
    border-right: 1px solid oklch(88% 0.006 250);
  }

  .song-grid-header > div:last-child {
    border-right: none;
  }

  .song-grid-row {
    border-bottom: 1px solid oklch(94% 0.003 250);
    background: white;
    color: oklch(22% 0.01 250);
  }

  .song-grid-row.even {
    background: oklch(97.5% 0.002 250);
  }

  .song-grid-row:hover {
    background: oklch(94% 0.008 250);
  }

  .song-grid-row.selected {
    background: linear-gradient(
      to bottom,
      oklch(58% 0.2 250),
      oklch(52% 0.22 250)
    );
    color: white;
  }

  .song-grid-row.selected > div {
    text-shadow: 0 1px 1px oklch(0% 0 0 / 0.25);
  }

  .song-grid-row.playing {
    background: linear-gradient(
      to bottom,
      oklch(62% 0.18 250),
      oklch(55% 0.2 250)
    );
    color: white;
    font-weight: 500;
  }

  .song-grid-row > div {
    padding: 0.25rem 0.75rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .song-grid-row .truncate {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dimmed {
    color: oklch(50% 0.01 250);
  }

  .song-grid-row.selected .dimmed,
  .song-grid-row.playing .dimmed {
    color: oklch(90% 0 0 / 0.7);
  }

  .cell-checkbox {
    justify-self: center;
    padding-left: 0.5rem !important;
    padding-right: 0.5rem !important;
  }

  .cell-time {
    text-align: right;
  }

  .cell-year,
  .cell-genre {
    padding-left: 1rem !important;
  }
</style>
