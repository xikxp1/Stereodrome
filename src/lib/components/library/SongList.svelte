<script lang="ts">
  import type { Song } from "$lib/types";
  import { playlistStore } from "$lib/stores/playlist.svelte";
  import { createVirtualizer } from "@tanstack/svelte-virtual";
  import {
    CircleAlert,
    Music,
    Volume2,
    ListPlus,
    ListX,
    Plus,
  } from "lucide-svelte";

  interface Props {
    songs?: Song[];
    isLoading?: boolean;
    error?: Error | null;
    selectedSongId?: string | null;
    playingSongId?: string | null;
    scrollToSongId?: string | null;
    playlistId?: string | null;
    onSelect?: (song: Song) => void;
    onPlay?: (song: Song) => void;
  }

  let {
    songs = [],
    isLoading = false,
    error = null,
    selectedSongId = null,
    playingSongId = null,
    scrollToSongId = null,
    playlistId = null,
    onSelect,
    onPlay,
  }: Props = $props();

  let scrollContainer: HTMLDivElement | null = $state(null);

  const ROW_HEIGHT = 24;

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

  // Track previous scrollToSongId to detect changes
  let prevScrollToSongId: string | null = $state(null);

  // Scroll to song when scrollToSongId changes
  $effect(() => {
    if (
      scrollToSongId &&
      scrollToSongId !== prevScrollToSongId &&
      scrollContainer
    ) {
      const index = songs.findIndex((s) => s.id === scrollToSongId);
      if (index >= 0) {
        requestAnimationFrame(() => {
          $virtualizer.scrollToIndex(index, {
            align: "center",
            behavior: "smooth",
          });
        });
      }
    }
    prevScrollToSongId = scrollToSongId;
  });

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

  // Context menu state
  let contextMenu = $state<{
    x: number;
    y: number;
    song: Song;
  } | null>(null);

  let showNewPlaylistInput = $state(false);
  let newPlaylistName = $state("");

  const availablePlaylists = $derived(
    playlistStore.playlists.filter((p) => p.id !== playlistId)
  );

  // Close context menu on click outside
  $effect(() => {
    if (contextMenu) {
      const handler = () => {
        contextMenu = null;
        showNewPlaylistInput = false;
        newPlaylistName = "";
      };
      window.addEventListener("click", handler);
      return () => window.removeEventListener("click", handler);
    }
  });

  function handleRowContextMenu(e: MouseEvent, song: Song) {
    e.preventDefault();
    showNewPlaylistInput = false;
    newPlaylistName = "";
    contextMenu = { x: e.clientX, y: e.clientY, song };
  }

  async function addToPlaylist(playlistId: string) {
    if (!contextMenu) return;
    await playlistStore.addSongsToPlaylist(playlistId, [contextMenu.song.id]);
    contextMenu = null;
  }

  async function removeFromPlaylist() {
    if (!contextMenu || !playlistId) return;
    const index = songs.indexOf(contextMenu.song);
    if (index >= 0) {
      await playlistStore.removeSongFromPlaylist(playlistId, index);
    }
    contextMenu = null;
  }

  async function createPlaylistWithSong() {
    if (!newPlaylistName.trim() || !contextMenu) return;
    const playlist = await playlistStore.createPlaylist(
      newPlaylistName.trim(),
      [contextMenu.song.id]
    );
    if (playlist) {
      newPlaylistName = "";
      showNewPlaylistInput = false;
      contextMenu = null;
    }
  }
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
      <CircleAlert class="empty-state-icon" />
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
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="song-grid-row"
              class:selected={selectedSongId === song.id}
              class:playing={playingSongId === song.id}
              class:even={index % 2 === 1}
              onclick={() => handleRowClick(song)}
              ondblclick={() => handleRowDoubleClick(song)}
              oncontextmenu={(e) => handleRowContextMenu(e, song)}
              style="position: absolute; top: 0; left: 0; width: 100%; height: {row.size}px; transform: translateY({row.start}px);"
            >
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

<!-- Song context menu -->
{#if contextMenu}
  <div
    class="ctx-menu"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
  >
    {#if playlistId}
      <button
        class="ctx-item"
        onclick={(e) => {
          e.stopPropagation();
          removeFromPlaylist();
        }}
      >
        <ListX class="size-3" />
        Remove from Playlist
      </button>
      <div class="ctx-divider"></div>
    {/if}
    {#if availablePlaylists.length > 0}
      <div class="ctx-label">
        <ListPlus class="size-3" />
        Add to Playlist
      </div>
      {#each availablePlaylists as playlist (playlist.id)}
        <button
          class="ctx-item"
          onclick={(e) => {
            e.stopPropagation();
            addToPlaylist(playlist.id);
          }}
        >
          {playlist.name}
        </button>
      {/each}
      <div class="ctx-divider"></div>
    {/if}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    {#if showNewPlaylistInput}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="ctx-input-wrap" onclick={(e) => e.stopPropagation()}>
        <input
          type="text"
          class="ctx-input"
          placeholder="Playlist name..."
          autocomplete="off"
          autocorrect="off"
          autocapitalize="off"
          spellcheck="false"
          bind:value={newPlaylistName}
          onkeydown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") createPlaylistWithSong();
            if (e.key === "Escape") {
              showNewPlaylistInput = false;
              newPlaylistName = "";
            }
          }}
        />
      </div>
    {:else}
      <button
        class="ctx-item"
        onclick={(e) => {
          e.stopPropagation();
          showNewPlaylistInput = true;
        }}
      >
        <Plus class="size-3" />
        New Playlist...
      </button>
    {/if}
  </div>
{/if}

<style>
  .song-list-container {
    container-type: inline-size;
  }

  .song-grid-header,
  .song-grid-row {
    display: grid;
    grid-template-columns:
      40px minmax(120px, 1fr) 52px minmax(100px, 0.6fr)
      minmax(100px, 0.8fr) 56px 96px;
    align-items: center;
    font-size: 0.75rem;
  }

  /* Hide genre column below 900px */
  @container (max-width: 900px) {
    .song-grid-header,
    .song-grid-row {
      grid-template-columns:
        40px minmax(120px, 1fr) 52px minmax(100px, 0.6fr)
        minmax(100px, 0.8fr) 56px;
    }

    .cell-genre {
      display: none;
    }
  }

  /* Hide year and genre columns below 800px */
  @container (max-width: 800px) {
    .song-grid-header,
    .song-grid-row {
      grid-template-columns:
        40px minmax(120px, 1fr) 52px minmax(80px, 0.6fr)
        minmax(80px, 0.8fr);
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
    background: oklch(92% 0.04 250);
    color: oklch(22% 0.01 250);
    font-weight: 500;
  }

  .song-grid-row.playing.selected {
    background: linear-gradient(
      to bottom,
      oklch(58% 0.2 250),
      oklch(52% 0.22 250)
    );
    color: white;
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

  .song-grid-row.selected .dimmed {
    color: oklch(90% 0 0 / 0.7);
  }

  .cell-time {
    text-align: right;
  }

  .cell-year,
  .cell-genre {
    padding-left: 1rem !important;
  }

  /* Context menu - macOS-native feel matching iTunes aesthetic */
  .ctx-menu {
    position: fixed;
    z-index: 1000;
    min-width: 180px;
    max-width: 260px;
    max-height: 320px;
    overflow-y: auto;
    padding: 4px;
    background: oklch(98% 0.002 250);
    border: 1px solid oklch(82% 0.008 250);
    border-radius: 6px;
    box-shadow:
      0 6px 20px oklch(0% 0 0 / 0.15),
      0 1px 3px oklch(0% 0 0 / 0.1);
  }

  .ctx-label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    font-size: 0.6875rem;
    font-weight: 600;
    text-transform: uppercase;
    color: oklch(50% 0.01 250);
    letter-spacing: 0.03em;
  }

  .ctx-item {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 4px 8px;
    font-size: 0.75rem;
    line-height: 1.25;
    text-align: left;
    color: oklch(22% 0.01 250);
    border: none;
    background: transparent;
    border-radius: 4px;
    cursor: default;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ctx-item:hover {
    background: linear-gradient(
      to bottom,
      oklch(58% 0.2 250),
      oklch(52% 0.22 250)
    );
    color: white;
  }

  .ctx-divider {
    height: 1px;
    margin: 4px 8px;
    background: oklch(88% 0.006 250);
  }

  .ctx-input-wrap {
    padding: 4px 6px;
  }

  .ctx-input {
    width: 100%;
    padding: 3px 6px;
    font-size: 0.6875rem;
    color: oklch(22% 0.01 250);
    background: white;
    border: 1px solid oklch(82% 0.008 250);
    border-radius: 4px;
    outline: none;
  }

  .ctx-input:focus {
    border-color: oklch(55% 0.22 250);
    box-shadow: 0 0 0 2px oklch(55% 0.22 250 / 0.2);
  }
</style>
