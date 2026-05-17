<script lang="ts">
  import type { Song } from "$lib/types";
  import { playlistStore } from "$lib/stores/playlist.svelte";
  import { queue } from "$lib/stores/queue.svelte";
  import { createVirtualizer } from "@tanstack/svelte-virtual";
  import { CircleAlert, Download, Music, Volume2 } from "lucide-svelte";
  import { showSongContextMenu } from "$lib/services/contextMenu";

  type SongRowKey = string;
  type PlaylistSong = Song & { position?: number };

  interface Props {
    songs?: Song[];
    isLoading?: boolean;
    error?: Error | null;
    selectedSongId?: string | null;
    playingSongId?: string | null;
    scrollToSongId?: string | null;
    playlistId?: string | null;
    downloadedSongIds?: ReadonlySet<string>;
    onSelect?: (song: Song) => void;
    onPlay?: (song: Song) => void;
    onNavigateToArtist?: (song: Song) => void;
    onNavigateToAlbum?: (song: Song) => void;
  }

  let {
    songs = [],
    isLoading = false,
    error = null,
    selectedSongId = null,
    playingSongId = null,
    scrollToSongId = null,
    playlistId = null,
    downloadedSongIds = new Set(),
    onSelect,
    onPlay,
    onNavigateToArtist,
    onNavigateToAlbum,
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

  let selectedSongKeys = $state<SongRowKey[]>([]);
  let orderedSelectedSongKeys = $state<SongRowKey[]>([]);
  let selectionAnchorKey = $state<SongRowKey | null>(null);
  let pendingInternalSelectedSongId = $state<string | null>(null);
  let lastObservedSelectedSongId = $state<string | null | undefined>(undefined);

  const songByKey = $derived.by(
    () =>
      new Map(
        songs.map((song, index) => [getSongRowKey(song, index), song] as const)
      )
  );
  const selectedSongLookup = $derived.by(
    () =>
      Object.fromEntries(
        selectedSongKeys.map((songKey) => [songKey, true] as const)
      ) as Record<SongRowKey, boolean>
  );
  const selectedSongs = $derived.by(() =>
    orderedSelectedSongKeys
      .map((songKey) => songByKey.get(songKey))
      .filter((song): song is Song => song !== undefined)
  );

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

  function arraysEqual(a: string[], b: string[]) {
    return (
      a.length === b.length && a.every((value, index) => value === b[index])
    );
  }

  function getSongRowKey(song: Song, index: number): SongRowKey {
    const position = (song as PlaylistSong).position;

    if (playlistId && typeof position === "number") {
      return `playlist-position:${position}`;
    }

    if (playlistId) {
      return `playlist-row:${index}:${song.id}`;
    }

    return song.id;
  }

  function getFirstSongRowKeyById(songId: string): SongRowKey | null {
    const index = songs.findIndex((song) => song.id === songId);
    return index >= 0 ? getSongRowKey(songs[index], index) : null;
  }

  function replaceSelection(
    nextSelectedSongKeys: SongRowKey[],
    nextOrderedSongKeys: SongRowKey[],
    nextAnchorKey: SongRowKey | null
  ) {
    selectedSongKeys = nextSelectedSongKeys;
    orderedSelectedSongKeys = nextOrderedSongKeys;
    selectionAnchorKey = nextAnchorKey;
  }

  function focusSong(song: Song) {
    if (!onSelect) {
      return;
    }

    pendingInternalSelectedSongId = song.id;
    onSelect(song);
  }

  function getSelectedKeysInListOrder(songKeys: SongRowKey[]) {
    return songs
      .map((song, index) => getSongRowKey(song, index))
      .filter((songKey) => songKeys.includes(songKey));
  }

  function selectOnlySong(song: Song, index: number) {
    const songKey = getSongRowKey(song, index);
    replaceSelection([songKey], [songKey], songKey);
  }

  function toggleSongSelection(song: Song, index: number) {
    const songKey = getSongRowKey(song, index);

    if (selectedSongKeys.includes(songKey)) {
      replaceSelection(
        selectedSongKeys.filter((selectedKey) => selectedKey !== songKey),
        orderedSelectedSongKeys.filter(
          (selectedKey) => selectedKey !== songKey
        ),
        songKey
      );
      return;
    }

    replaceSelection(
      getSelectedKeysInListOrder([...selectedSongKeys, songKey]),
      [
        ...orderedSelectedSongKeys.filter(
          (selectedKey) => selectedKey !== songKey
        ),
        songKey,
      ],
      songKey
    );
  }

  function getRangeSongKeys(anchorKey: SongRowKey, targetKey: SongRowKey) {
    const anchorIndex = songs.findIndex(
      (song, index) => getSongRowKey(song, index) === anchorKey
    );
    const targetIndex = songs.findIndex(
      (song, index) => getSongRowKey(song, index) === targetKey
    );

    if (anchorIndex < 0 || targetIndex < 0) {
      return [targetKey];
    }

    const start = Math.min(anchorIndex, targetIndex);
    const end = Math.max(anchorIndex, targetIndex);
    return songs
      .slice(start, end + 1)
      .map((song, offset) => getSongRowKey(song, start + offset));
  }

  function selectSongRange(song: Song, index: number) {
    const songKey = getSongRowKey(song, index);
    const anchorKey =
      selectionAnchorKey ??
      (selectedSongId ? getFirstSongRowKeyById(selectedSongId) : null) ??
      songKey;
    const rangeSongKeys = getRangeSongKeys(anchorKey, songKey);
    const nextOrderedSongKeys = [
      ...orderedSelectedSongKeys.filter((selectedKey) =>
        rangeSongKeys.includes(selectedKey)
      ),
      ...rangeSongKeys.filter((songKey) => !selectedSongKeys.includes(songKey)),
    ];

    replaceSelection(rangeSongKeys, nextOrderedSongKeys, anchorKey);
  }

  function collapseSelectionToSongId(songId: string | null) {
    const songKey = songId ? getFirstSongRowKeyById(songId) : null;

    if (!songKey) {
      replaceSelection([], [], null);
      return;
    }

    replaceSelection([songKey], [songKey], songKey);
  }

  function handleRowClick(event: MouseEvent, song: Song, index: number) {
    if (event.shiftKey) {
      selectSongRange(song, index);
    } else if (event.metaKey || event.ctrlKey) {
      toggleSongSelection(song, index);
    } else {
      selectOnlySong(song, index);
    }

    focusSong(song);
  }

  function handleRowDoubleClick(song: Song) {
    onPlay?.(song);
  }

  // New playlist dialog state
  let newPlaylistSongIds = $state<string[]>([]);
  let newPlaylistName = $state("");
  let newPlaylistDialog: HTMLDialogElement | undefined = $state();

  const availablePlaylists = $derived(
    playlistStore.playlists.filter((p) => p.id !== playlistId)
  );

  function getPlaylistPositions(songsToRemove: Song[]) {
    return songsToRemove
      .map((song) => {
        const position = (song as PlaylistSong).position;
        return typeof position === "number" ? position : null;
      })
      .filter((position): position is number => position !== null);
  }

  async function handleRowContextMenu(
    e: MouseEvent,
    song: Song,
    index: number
  ) {
    e.preventDefault();

    const songKey = getSongRowKey(song, index);
    const isExistingSelection = !!selectedSongLookup[songKey];
    const contextSelectedSongs = isExistingSelection
      ? selectedSongs.length > 0
        ? selectedSongs
        : [song]
      : [song];

    if (!isExistingSelection) {
      selectOnlySong(song, index);
    }

    focusSong(song);

    const isMultiSelect = contextSelectedSongs.length > 1;
    const primarySong = contextSelectedSongs[0];

    await showSongContextMenu({
      selectionCount: contextSelectedSongs.length,
      playlists: availablePlaylists.map((p) => ({ id: p.id, name: p.name })),
      onPlayNext: async () => {
        await queue.playNextSongs(contextSelectedSongs);
      },
      onAddToQueue: async () => {
        await queue.addSongs(contextSelectedSongs);
      },
      showGoToArtist: isMultiSelect || !!primarySong?.artist_id,
      showGoToAlbum: isMultiSelect || !!primarySong?.album_id,
      disableGoToArtist: isMultiSelect,
      disableGoToAlbum: isMultiSelect,
      onGoToArtist:
        !isMultiSelect && primarySong?.artist_id
          ? () => {
              onNavigateToArtist?.(primarySong);
            }
          : undefined,
      onGoToAlbum:
        !isMultiSelect && primarySong?.album_id
          ? () => {
              onNavigateToAlbum?.(primarySong);
            }
          : undefined,
      onRemoveFromPlaylist: playlistId
        ? async () => {
            const positions = getPlaylistPositions(contextSelectedSongs);
            if (positions.length > 0) {
              await playlistStore.removeSongsFromPlaylist(
                playlistId,
                positions
              );
            }
          }
        : undefined,
      onAddToPlaylist: async (targetPlaylistId: string) => {
        await playlistStore.addSongsToPlaylist(
          targetPlaylistId,
          contextSelectedSongs.map((selectedSong) => selectedSong.id)
        );
      },
      onNewPlaylist: () => {
        newPlaylistSongIds = contextSelectedSongs.map(
          (selectedSong) => selectedSong.id
        );
        newPlaylistName = "";
        newPlaylistDialog?.showModal();
      },
    });
  }

  $effect(() => {
    const availableSongKeys = new Set(
      songs.map((song, index) => getSongRowKey(song, index))
    );
    const nextSelectedSongKeys = selectedSongKeys.filter((songKey) =>
      availableSongKeys.has(songKey)
    );
    const nextOrderedSongKeys = orderedSelectedSongKeys.filter((songKey) =>
      availableSongKeys.has(songKey)
    );
    const nextAnchorKey =
      selectionAnchorKey && availableSongKeys.has(selectionAnchorKey)
        ? selectionAnchorKey
        : null;

    if (!arraysEqual(selectedSongKeys, nextSelectedSongKeys)) {
      selectedSongKeys = nextSelectedSongKeys;
    }
    if (!arraysEqual(orderedSelectedSongKeys, nextOrderedSongKeys)) {
      orderedSelectedSongKeys = nextOrderedSongKeys;
    }
    if (selectionAnchorKey !== nextAnchorKey) {
      selectionAnchorKey = nextAnchorKey;
    }
  });

  $effect(() => {
    if (selectedSongId === lastObservedSelectedSongId) {
      return;
    }

    lastObservedSelectedSongId = selectedSongId;

    if (
      selectedSongId !== null &&
      selectedSongId === pendingInternalSelectedSongId
    ) {
      pendingInternalSelectedSongId = null;
      return;
    }

    pendingInternalSelectedSongId = null;
    collapseSelectionToSongId(selectedSongId);
  });

  async function handleCreatePlaylistWithSongs() {
    if (!newPlaylistName.trim() || newPlaylistSongIds.length === 0) return;
    await playlistStore.createPlaylist(
      newPlaylistName.trim(),
      newPlaylistSongIds
    );
    newPlaylistName = "";
    newPlaylistSongIds = [];
    newPlaylistDialog?.close();
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
        <div class="cell-download" title="Downloaded">
          <Download class="h-3 w-3" />
        </div>
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
              class:selected={!!selectedSongLookup[getSongRowKey(song, index)]}
              class:playing={playingSongId === song.id}
              class:even={index % 2 === 1}
              onclick={(event) => handleRowClick(event, song, index)}
              ondblclick={() => handleRowDoubleClick(song)}
              oncontextmenu={(e) => handleRowContextMenu(e, song, index)}
              style="position: absolute; top: 0; left: 0; width: 100%; height: {row.size}px; transform: translateY({row.start}px);"
            >
              <div class="cell-track dimmed">
                {song.track_number || index + 1}
              </div>
              <div class="cell-download">
                {#if downloadedSongIds.has(song.id)}
                  <span class="downloaded-icon">
                    <Download class="h-3 w-3" />
                  </span>
                  <span class="sr-only">Downloaded</span>
                {:else}
                  <span class="not-downloaded" title="Not downloaded">-</span>
                {/if}
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

<!-- New playlist dialog -->
<dialog bind:this={newPlaylistDialog} class="modal">
  <div class="modal-box w-72 p-4">
    <h3 class="text-sm font-semibold mb-2">New Playlist</h3>
    <input
      type="text"
      class="input input-bordered input-sm w-full"
      placeholder="Playlist name..."
      autocomplete="off"
      autocorrect="off"
      autocapitalize="off"
      spellcheck="false"
      bind:value={newPlaylistName}
      onkeydown={(e) => {
        if (e.key === "Enter") handleCreatePlaylistWithSongs();
        if (e.key === "Escape") newPlaylistDialog?.close();
      }}
    />
    <div class="modal-action mt-3">
      <button class="btn btn-sm" onclick={() => newPlaylistDialog?.close()}>
        Cancel
      </button>
      <button
        class="btn btn-sm btn-primary"
        onclick={handleCreatePlaylistWithSongs}
      >
        Create
      </button>
    </div>
  </div>
  <form method="dialog" class="modal-backdrop">
    <button>close</button>
  </form>
</dialog>

<style>
  .song-list-container {
    container-type: inline-size;
  }

  .song-grid-header,
  .song-grid-row {
    display: grid;
    grid-template-columns:
      40px 28px minmax(120px, 1fr) 52px minmax(100px, 0.6fr)
      minmax(100px, 0.8fr) 56px 96px;
    align-items: center;
    font-size: 0.75rem;
  }

  /* Hide genre column below 900px */
  @container (max-width: 900px) {
    .song-grid-header,
    .song-grid-row {
      grid-template-columns:
        40px 28px minmax(120px, 1fr) 52px minmax(100px, 0.6fr)
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
        40px 28px minmax(120px, 1fr) 52px minmax(80px, 0.6fr)
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

  .cell-download {
    display: flex;
    justify-content: center;
    padding-left: 0 !important;
    padding-right: 0 !important;
  }

  .downloaded-icon {
    color: oklch(58% 0.16 150);
  }

  .not-downloaded {
    color: oklch(65% 0.01 250);
    font-size: 0.6875rem;
  }

  .song-grid-row.selected .downloaded-icon,
  .song-grid-row.selected .not-downloaded {
    color: currentColor;
  }

  .cell-year,
  .cell-genre {
    padding-left: 1rem !important;
  }
</style>
