<script lang="ts">
  import ServerConnect from "$lib/components/ServerConnect.svelte";
  import TransportBar from "$lib/components/TransportBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ColumnBrowser from "$lib/components/library/ColumnBrowser.svelte";
  import SongList from "$lib/components/library/SongList.svelte";
  import ArtistGridView from "$lib/components/library/ArtistGridView.svelte";
  import AlbumGridView from "$lib/components/library/AlbumGridView.svelte";
  import DetailHeader from "$lib/components/library/DetailHeader.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import QueuePanel from "$lib/components/QueuePanel.svelte";
  import SettingsModal from "$lib/components/SettingsModal.svelte";
  import { connection } from "$lib/stores/connection.svelte";
  import { playback } from "$lib/stores/playback.svelte";
  import { queue } from "$lib/stores/queue.svelte";
  import { searchStore } from "$lib/stores/search.svelte";
  import { spectrum } from "$lib/stores/spectrum.svelte";
  import { updater } from "$lib/stores/updater.svelte";
  import {
    getArtists,
    getAlbums,
    getSongs,
    getLibrarySyncStatus,
    getSystemTimePreferences,
    seekPlayback,
    getCoverArt,
    getMiniPlayerPosition,
    openMiniPlayer,
  } from "$lib/api/commands";
  import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    availableMonitors,
    currentMonitor,
    type Monitor,
  } from "@tauri-apps/api/window";
  import { error } from "@tauri-apps/plugin-log";
  import { playlistStore } from "$lib/stores/playlist.svelte";
  import type {
    Artist,
    Album,
    Song,
    Playlist,
    MiniPlayerPosition,
    LibrarySyncStatus,
  } from "$lib/types";

  // View state
  let activeView = $state("music");
  let detailView = $state<{
    type: "artist" | "album";
    artist?: Artist;
    album?: Album;
  } | null>(null);
  let queueOpen = $state(false);
  let settingsOpen = $state(false);
  let selectedPlaylist = $state<Playlist | null>(null);

  // Keyboard shortcut state
  let previousVolume = $state(100); // For mute/unmute toggle (0-100 scale)
  let volumeAdjusting = $state(false);
  let volumeAdjustTimeout: ReturnType<typeof setTimeout> | null = null;
  let searchInputRef = $state<HTMLInputElement | null>(null);

  // Browser state
  let artists = $state<Artist[]>([]);
  let albums = $state<Album[]>([]);
  let songs = $state<Song[]>([]);

  let selectedGenre = $state<string | null>(null);
  let selectedArtist = $state<Artist | null>(null);
  let selectedAlbum = $state<Album | null>(null);
  let selectedSong = $state<Song | null>(null);
  let scrollToSongId = $state<string | null>(null);

  // Get current track from local playback state (no server latency)
  const currentTrack = $derived(playback.currentTrack);
  const volume = $derived(Math.round(playback.volume * 100)); // Convert to 0-100 for UI

  // Cover art state
  let coverArtUrl = $state<string | null>(null);
  let lastCoverArtId = $state<string | null>(null);

  const MINI_PLAYER_WIDTH = 320;
  const MINI_PLAYER_HEIGHT = 72;
  const MINI_PLAYER_MARGIN = 8;

  interface LogicalMonitorBounds {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  // Fetch cover art thumbnail when track changes
  $effect(() => {
    const coverArtId = currentTrack?.coverArtId;
    if (coverArtId && coverArtId !== lastCoverArtId) {
      lastCoverArtId = coverArtId;
      // Fetch thumbnail (64px for transport bar)
      getCoverArt(coverArtId, 64)
        .then((url) => {
          coverArtUrl = url;
        })
        .catch((e) => {
          error(`Failed to fetch cover art thumbnail: ${e}`);
          coverArtUrl = null;
        });
    } else if (!coverArtId) {
      coverArtUrl = null;
      lastCoverArtId = null;
    }
  });

  // Loading states
  let isLoading = $state(false);
  let loadError = $state<Error | null>(null);
  let librarySyncStatus = $state<LibrarySyncStatus | null>(null);
  let systemLocale = $state<string | null>(null);
  let use24HourClock = $state<boolean | null>(null);
  const syncDateTimeFormatter = $derived.by(() => {
    const locale = systemLocale ?? undefined;
    const options: Intl.DateTimeFormatOptions = {
      dateStyle: "short",
      timeStyle: "short",
    };
    if (use24HourClock !== null) {
      options.hour12 = !use24HourClock;
    }
    return new Intl.DateTimeFormat(locale, options);
  });

  // Restore session on mount (runs once)
  let sessionRestored = false;
  $effect(() => {
    if (!sessionRestored) {
      sessionRestored = true;
      connection.restore();
    }
  });

  // Check for updates on startup (runs once after connection)
  let updateChecked = false;
  $effect(() => {
    if (connection.status.connected && !updateChecked) {
      updateChecked = true;
      // Non-blocking update check
      updater.checkForUpdate();
    }
  });

  // Listen for open-settings event from tray
  $effect(() => {
    const handler = () => {
      settingsOpen = true;
    };
    window.addEventListener("open-settings", handler);
    return () => window.removeEventListener("open-settings", handler);
  });

  // Load data when connected
  $effect(() => {
    if (connection.status.connected) {
      loadLibraryData();
    }
  });

  let timePreferencesLoaded = false;
  $effect(() => {
    if (timePreferencesLoaded) return;
    timePreferencesLoaded = true;
    void loadSystemTimePreferences();
  });

  // Keep library sync status current in main window.
  $effect(() => {
    let unlistenStatus: UnlistenFn | null = null;
    let unlistenSettings: UnlistenFn | null = null;

    (async () => {
      try {
        unlistenStatus = await listen<LibrarySyncStatus>(
          "library-sync-status-changed",
          (event) => {
            librarySyncStatus = event.payload;
          }
        );
      } catch (e) {
        error(`Failed to listen for library sync status events: ${e}`);
      }

      try {
        unlistenSettings = await listen("sync-settings-changed", () => {
          void loadLibrarySyncStatus();
        });
      } catch (e) {
        error(`Failed to listen for sync settings events: ${e}`);
      }
    })();

    return () => {
      if (unlistenStatus) unlistenStatus();
      if (unlistenSettings) unlistenSettings();
    };
  });

  $effect(() => {
    if (!connection.status.connected) {
      librarySyncStatus = null;
      return;
    }

    void loadLibrarySyncStatus();
    const interval = setInterval(() => {
      void loadLibrarySyncStatus();
    }, 60_000);

    return () => {
      clearInterval(interval);
    };
  });

  async function loadLibraryData() {
    isLoading = true;
    loadError = null;
    try {
      const [artistsData, albumsData, songsData] = await Promise.all([
        getArtists(),
        getAlbums(),
        getSongs(),
      ]);
      artists = artistsData;
      albums = albumsData;
      songs = songsData;
    } catch (e) {
      loadError = e instanceof Error ? e : new Error(String(e));
    } finally {
      isLoading = false;
    }
  }

  async function loadLibrarySyncStatus() {
    if (!connection.status.connected) return;
    try {
      librarySyncStatus = await getLibrarySyncStatus();
    } catch (e) {
      error(`Failed to load library sync status: ${e}`);
    }
  }

  async function loadSystemTimePreferences() {
    try {
      const preferences = await getSystemTimePreferences();
      use24HourClock = preferences.use_24_hour_clock;
      systemLocale = preferences.locale;
    } catch (e) {
      error(`Failed to load system time preferences: ${e}`);
      use24HourClock = null;
      systemLocale = null;
    }
  }

  function formatSyncTimestamp(value: string): string {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) {
      return "?";
    }
    return syncDateTimeFormatter.format(date);
  }

  const syncStatusTone = $derived.by((): "normal" | "running" | "error" => {
    if (!connection.status.connected) return "normal";
    if (!librarySyncStatus) return "normal";
    if (librarySyncStatus.active_job) return "running";
    if (
      librarySyncStatus.incremental.last_error ||
      librarySyncStatus.full_reconcile.last_error
    ) {
      return "error";
    }
    return "normal";
  });

  const syncStatusSummary = $derived.by((): string | null => {
    if (!connection.status.connected) return null;
    if (!librarySyncStatus) return "Sync: unavailable";

    if (librarySyncStatus.active_job === "incremental") {
      return "Sync: incremental running";
    }
    if (librarySyncStatus.active_job === "full_reconcile") {
      return "Sync: full reconcile running";
    }

    if (librarySyncStatus.incremental.last_error) {
      return "Sync: incremental error (see Settings)";
    }
    if (librarySyncStatus.full_reconcile.last_error) {
      return "Sync: full reconcile error (see Settings)";
    }

    const incrementalNext = librarySyncStatus.incremental.next_run_at
      ? formatSyncTimestamp(librarySyncStatus.incremental.next_run_at)
      : "off";
    const fullNext = librarySyncStatus.full_reconcile.next_run_at
      ? formatSyncTimestamp(librarySyncStatus.full_reconcile.next_run_at)
      : "off";
    return `Sync: inc ${incrementalNext} | full ${fullNext}`;
  });

  function getLogicalMonitorBounds(monitor: Monitor): LogicalMonitorBounds {
    const scale = monitor.scaleFactor || 1;
    return {
      x: monitor.workArea.position.x / scale,
      y: monitor.workArea.position.y / scale,
      width: monitor.workArea.size.width / scale,
      height: monitor.workArea.size.height / scale,
    };
  }

  function clampPositionToMonitor(
    position: MiniPlayerPosition,
    bounds: LogicalMonitorBounds
  ): MiniPlayerPosition {
    const maxX = bounds.x + Math.max(0, bounds.width - MINI_PLAYER_WIDTH);
    const maxY = bounds.y + Math.max(0, bounds.height - MINI_PLAYER_HEIGHT);

    return {
      x: Math.round(Math.min(Math.max(position.x, bounds.x), maxX)),
      y: Math.round(Math.min(Math.max(position.y, bounds.y), maxY)),
    };
  }

  function monitorContainsPosition(
    bounds: LogicalMonitorBounds,
    position: MiniPlayerPosition
  ): boolean {
    return (
      position.x >= bounds.x &&
      position.y >= bounds.y &&
      position.x < bounds.x + bounds.width &&
      position.y < bounds.y + bounds.height
    );
  }

  function getDefaultMiniPlayerPosition(bounds: LogicalMonitorBounds) {
    const maxX = bounds.x + Math.max(0, bounds.width - MINI_PLAYER_WIDTH);
    const x = Math.max(bounds.x, Math.round(maxX - MINI_PLAYER_MARGIN));
    const y = Math.round(
      Math.min(
        bounds.y + MINI_PLAYER_MARGIN,
        bounds.y + Math.max(0, bounds.height - MINI_PLAYER_HEIGHT)
      )
    );
    return { x, y };
  }

  async function resolveMiniPlayerPosition(): Promise<MiniPlayerPosition> {
    const [monitors, activeMonitor] = await Promise.all([
      availableMonitors(),
      currentMonitor(),
    ]);
    const fallbackMonitor = activeMonitor ?? monitors[0] ?? null;

    if (!fallbackMonitor) {
      return { x: 100, y: 100 };
    }

    const fallbackBounds = getLogicalMonitorBounds(fallbackMonitor);
    let savedPosition: MiniPlayerPosition | null = null;

    try {
      savedPosition = await getMiniPlayerPosition();
    } catch (e) {
      error(`Failed to load mini player position: ${e}`);
    }

    if (savedPosition) {
      const boundsForSavedPosition = monitors
        .map(getLogicalMonitorBounds)
        .find((bounds) => monitorContainsPosition(bounds, savedPosition));
      const targetBounds = boundsForSavedPosition ?? fallbackBounds;
      return clampPositionToMonitor(savedPosition, targetBounds);
    }

    return getDefaultMiniPlayerPosition(fallbackBounds);
  }

  // Compute filtered data - all columns derived from shown songs
  const filterResult = $derived.by(() => {
    const hasSearch = searchStore.hasActiveQuery;
    const genre = selectedGenre;
    const artist = selectedArtist;
    const album = selectedAlbum;
    const searchSongIds = searchStore.matchedSongIds;

    // Single pass through songs (these are temporary computation sets, not reactive state)
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const matchedGenres = new Set<string>();
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const matchedArtistIds = new Set<string>();
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const matchedAlbumIds = new Set<string>();
    const matchedSongs: Song[] = [];

    for (const s of songs) {
      // Apply all filters
      if (hasSearch && !searchSongIds.has(s.id)) continue;
      if (genre && s.genre !== genre) continue;
      if (artist && s.artist_id !== artist.id) continue;
      if (album && s.album_id !== album.id) continue;

      // Derive columns from shown songs
      if (s.genre) matchedGenres.add(s.genre);
      matchedArtistIds.add(s.artist_id);
      matchedAlbumIds.add(s.album_id);
      matchedSongs.push(s);
    }

    return {
      genres: [...matchedGenres].sort(),
      artistIds: matchedArtistIds,
      albumIds: matchedAlbumIds,
      songs: matchedSongs,
    };
  });

  // Derive from cached filterResult (no function call - direct property access)
  const filteredGenres = $derived(filterResult.genres);
  const filteredArtists = $derived(
    artists.filter((a) => filterResult.artistIds.has(a.id))
  );
  const filteredAlbums = $derived(
    albums.filter((a) => filterResult.albumIds.has(a.id))
  );
  const filteredSongs = $derived(filterResult.songs);

  // Filtered data for grid views (uses searchStore directly)
  const gridFilteredArtists = $derived(
    searchStore.hasActiveQuery
      ? artists.filter((a) => searchStore.matchedArtistIds.has(a.id))
      : artists
  );
  const gridFilteredAlbums = $derived(
    searchStore.hasActiveQuery
      ? albums.filter((a) => searchStore.matchedAlbumIds.has(a.id))
      : albums
  );

  // Songs for detail view
  const detailSongs = $derived.by(() => {
    const detail = detailView;
    if (!detail) return [];
    if (detail.type === "artist" && detail.artist) {
      const artistId = detail.artist.id;
      return songs.filter((s) => s.artist_id === artistId);
    }
    if (detail.type === "album" && detail.album) {
      const albumId = detail.album.id;
      return songs.filter((s) => s.album_id === albumId);
    }
    return [];
  });

  // Stats for detail view
  const detailTotalDuration = $derived(
    detailSongs.reduce((acc, s) => acc + (s.duration || 0), 0)
  );
  const detailTotalSize = $derived(
    detailSongs.reduce((acc, s) => acc + (s.size || 0), 0)
  );

  // Keep selectedPlaylist in sync with store (e.g., after rename)
  const selectedPlaylistData = $derived.by(() => {
    const current = selectedPlaylist;
    if (!current) return null;
    return playlistStore.playlists.find((p) => p.id === current.id) ?? current;
  });

  // Playlist view stats
  const playlistSongs = $derived(playlistStore.currentPlaylistSongs as Song[]);
  const filteredPlaylistSongs = $derived(
    searchStore.hasActiveQuery
      ? playlistSongs.filter((s) => searchStore.matchedSongIds.has(s.id))
      : playlistSongs
  );
  const playlistTotalDuration = $derived(
    filteredPlaylistSongs.reduce((acc, s) => acc + (s.duration || 0), 0)
  );
  const playlistTotalSize = $derived(
    filteredPlaylistSongs.reduce((acc, s) => acc + (s.size || 0), 0)
  );

  // Stats for status bar
  const totalDuration = $derived(
    filteredSongs.reduce((acc, s) => acc + (s.duration || 0), 0)
  );
  const totalSize = $derived(
    filteredSongs.reduce((acc, s) => acc + (s.size || 0), 0)
  );

  // Handlers - disabled during search
  function handleGenreSelect(genre: string | null) {
    if (searchStore.isSearching) return;
    selectedGenre = genre;
    selectedArtist = null;
    selectedAlbum = null;
  }

  function handleArtistSelect(artist: Artist | null) {
    if (searchStore.isSearching) return;
    selectedArtist = artist;
    selectedAlbum = null;
  }

  function handleAlbumSelect(album: Album | null) {
    if (searchStore.isSearching) return;
    selectedAlbum = album;
  }

  function handleSongSelect(song: Song) {
    if (searchStore.isSearching) return;
    selectedSong = song;
  }

  async function handleSongPlay(song: Song) {
    if (searchStore.isSearching) return;
    try {
      // Play with queue context - use filtered songs as the queue
      await queue.playSongWithQueue(song, filteredSongs);
    } catch (e) {
      error(`Failed to play song: ${e}`);
    }
  }

  async function handlePlayPause() {
    // If queue is empty, populate it based on current view
    if (queue.items.length === 0 && !playback.isPlaying) {
      const songsToPlay = selectedPlaylist
        ? filteredPlaylistSongs
        : detailView
          ? detailSongs
          : filteredSongs;
      if (songsToPlay.length > 0) {
        await queue.addSongs(songsToPlay);
        await queue.playQueueItem(0);
        return;
      }
    }
    playback.togglePlayPause();
  }

  function handleVolumeChange(v: number) {
    playback.setVolume(v / 100); // Convert from 0-100 to 0-1
    volumeAdjusting = true;
    if (volumeAdjustTimeout) clearTimeout(volumeAdjustTimeout);
    volumeAdjustTimeout = setTimeout(() => {
      volumeAdjusting = false;
    }, 1000);
  }

  function handleSeek(position: number) {
    seekPlayback(position);
  }

  function handlePrevious() {
    queue.playPrevious();
  }

  function handleNext() {
    queue.playNext();
  }

  function handleRerollNext() {
    queue.rerollNext();
  }

  function handleViewChange(view: string) {
    activeView = view;
    detailView = null; // Clear detail view when switching top-level views
    selectedPlaylist = null; // Deselect playlist when changing library views
  }

  function handlePlaylistSelect(playlist: Playlist | null) {
    selectedPlaylist = playlist;
    if (playlist) {
      playlistStore.selectPlaylist(playlist);
      detailView = null;
    } else {
      playlistStore.selectPlaylist(null);
    }
  }

  async function handlePlaylistSongPlay(song: Song) {
    try {
      await queue.playSongWithQueue(song, filteredPlaylistSongs);
    } catch (e) {
      error(`Failed to play song: ${e}`);
    }
  }

  function handleArtistGridSelect(artist: Artist) {
    detailView = { type: "artist", artist };
  }

  function handleAlbumGridSelect(album: Album) {
    detailView = { type: "album", album };
  }

  function handleDetailBack() {
    detailView = null;
  }

  async function handleDetailSongPlay(song: Song) {
    try {
      // Play with detail view songs as queue context
      await queue.playSongWithQueue(song, detailSongs);
    } catch (e) {
      error(`Failed to play song: ${e}`);
    }
  }

  function handleQueueToggle() {
    queueOpen = !queueOpen;
  }

  function handleSettingsToggle() {
    settingsOpen = !settingsOpen;
  }

  function handleSettingsClose() {
    settingsOpen = false;
  }

  async function handleMiniPlayerToggle() {
    try {
      const position = await resolveMiniPlayerPosition();
      await openMiniPlayer(position);
    } catch (e) {
      error(`Failed to open mini player: ${e}`);
    }
  }

  function handleQueueItemClick(songId: string) {
    // Find the song in the filtered list
    const song = filteredSongs.find((s) => s.id === songId);
    if (song) {
      // Select the song and scroll to it
      selectedSong = song;
      scrollToSongId = songId;
    }
  }

  async function handleCoverArtClick() {
    if (!currentTrack?.coverArtId) return;

    const coverArtData = {
      id: currentTrack.coverArtId,
      album: currentTrack.album,
      artist: currentTrack.artist,
    };

    // Check if window already exists
    const existingWindow = await WebviewWindow.getByLabel("cover-art-viewer");
    if (existingWindow) {
      // Update existing window with new cover art
      await emit("cover-art-update", coverArtData);
      await existingWindow.setTitle(
        `${currentTrack.artist}${currentTrack.album ? ` — ${currentTrack.album}` : ""}`
      );
      await existingWindow.setFocus();
      return;
    }

    // Create a new window for cover art viewing
    const webview = new WebviewWindow("cover-art-viewer", {
      url: `/cover-art?id=${encodeURIComponent(currentTrack.coverArtId)}&album=${encodeURIComponent(currentTrack.album)}&artist=${encodeURIComponent(currentTrack.artist)}`,
      title: `${currentTrack.artist}${currentTrack.album ? ` — ${currentTrack.album}` : ""}`,
      width: 500,
      height: 550,
      resizable: true,
      center: true,
      decorations: true,
    });

    webview.once("tauri://error", (e) => {
      error(`Failed to create cover art window: ${e}`);
    });
  }

  // Keyboard shortcuts handler
  function handleKeydown(event: KeyboardEvent) {
    const activeElement = document.activeElement;
    const isInputFocused =
      activeElement instanceof HTMLInputElement ||
      activeElement instanceof HTMLTextAreaElement;

    // Escape always works - blur input
    if (event.key === "Escape" && isInputFocused) {
      (activeElement as HTMLElement).blur();
      event.preventDefault();
      return;
    }

    // Skip other shortcuts when input is focused
    if (isInputFocused) return;

    const isMod = event.metaKey || event.ctrlKey;
    const isShift = event.shiftKey;

    switch (event.key) {
      case " ": // Space - play/pause
        event.preventDefault();
        handlePlayPause();
        break;

      case "Enter":
        // Play selected song
        if (selectedSong) {
          event.preventDefault();
          handleSongPlay(selectedSong);
        }
        break;

      case "ArrowLeft":
        event.preventDefault();
        if (isMod) {
          // Mod+Left - previous track
          handlePrevious();
        } else if (isShift) {
          // Shift+Left - seek back 10 seconds
          const newPos = Math.max(0, playback.position - 10);
          seekPlayback(newPos);
        }
        // Plain left arrow reserved for navigation (handled by components)
        break;

      case "ArrowRight":
        event.preventDefault();
        if (isMod) {
          // Mod+Right - next track
          handleNext();
        } else if (isShift) {
          // Shift+Right - seek forward 10 seconds
          const newPos = Math.min(playback.duration, playback.position + 10);
          seekPlayback(newPos);
        }
        // Plain right arrow reserved for navigation (handled by components)
        break;

      case "ArrowUp":
        if (isMod) {
          event.preventDefault();
          // Mod+Up - volume up 5%
          handleVolumeChange(Math.min(100, volume + 5));
        } else {
          // Plain up arrow - navigate to previous song in list
          event.preventDefault();
          navigateSongList(-1);
        }
        break;

      case "ArrowDown":
        if (isMod) {
          event.preventDefault();
          // Mod+Down - volume down 5%
          handleVolumeChange(Math.max(0, volume - 5));
        } else {
          // Plain down arrow - navigate to next song in list
          event.preventDefault();
          navigateSongList(1);
        }
        break;

      case "m":
      case "M":
        // Mute/unmute toggle
        if (volume > 0) {
          previousVolume = volume;
          handleVolumeChange(0);
        } else {
          handleVolumeChange(previousVolume);
        }
        break;

      case "s":
      case "S":
        if (!isMod && queue.items.length > 0) {
          // S - toggle shuffle (only when queue is not empty)
          queue.toggleShuffle();
        }
        break;

      case "r":
      case "R":
        if (!isMod && queue.items.length > 0) {
          // R - cycle repeat mode (only when queue is not empty)
          queue.cycleRepeatMode();
        }
        break;

      case "q":
      case "Q":
        if (!isMod) {
          // Q - toggle queue panel
          handleQueueToggle();
        }
        break;

      case "v":
      case "V":
        if (!isMod) {
          // V - toggle spectrum visualizer
          spectrum.toggle();
        }
        break;

      case "d":
      case "D":
        if (!isMod) {
          // D - reroll next track
          handleRerollNext();
        }
        break;

      case "k":
      case "K":
        if (isMod) {
          // Cmd/Ctrl+K - focus search
          event.preventDefault();
          searchInputRef?.focus();
        }
        break;

      case ",":
        if (isMod) {
          // Cmd/Ctrl+, - open settings
          event.preventDefault();
          handleSettingsToggle();
        }
        break;
    }
  }

  // Navigate up/down in the song list
  function navigateSongList(direction: number) {
    if (filteredSongs.length === 0) return;

    const selected = selectedSong;
    const currentIndex = selected
      ? filteredSongs.findIndex((s) => s.id === selected.id)
      : -1;

    let newIndex: number;
    if (currentIndex === -1) {
      // No selection - select first or last based on direction
      newIndex = direction > 0 ? 0 : filteredSongs.length - 1;
    } else {
      newIndex = currentIndex + direction;
      // Clamp to valid range
      if (newIndex < 0) newIndex = 0;
      if (newIndex >= filteredSongs.length) newIndex = filteredSongs.length - 1;
    }

    const newSong = filteredSongs[newIndex];
    if (newSong) {
      selectedSong = newSong;
      scrollToSongId = newSong.id;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="h-screen flex flex-col bg-base-200 overflow-hidden">
  {#if connection.status.connected}
    <!-- Transport Bar -->
    <TransportBar
      bind:searchInputRef
      isPlaying={playback.isPlaying}
      {currentTrack}
      currentTime={playback.position}
      duration={playback.duration}
      {volume}
      {volumeAdjusting}
      {queueOpen}
      {coverArtUrl}
      filteredSongsCount={filteredSongs.length}
      onPlayPause={handlePlayPause}
      onPrevious={handlePrevious}
      onNext={handleNext}
      onSeek={handleSeek}
      onVolumeChange={handleVolumeChange}
      onQueueToggle={handleQueueToggle}
      onCoverArtClick={handleCoverArtClick}
      onSettingsClick={handleSettingsToggle}
      onMiniPlayerToggle={handleMiniPlayerToggle}
    />

    <!-- Main Content Area -->
    <div class="flex-1 flex overflow-hidden">
      <!-- Sidebar -->
      <aside class="w-48 shrink-0">
        <Sidebar
          {activeView}
          onViewChange={handleViewChange}
          onPlaylistSelect={handlePlaylistSelect}
          selectedPlaylistId={selectedPlaylist?.id}
          onSync={loadLibraryData}
        />
      </aside>

      <!-- Content -->
      <main
        class="flex-1 flex flex-col overflow-hidden border-l border-base-300 transition-opacity duration-150"
        class:opacity-50={searchStore.isSearching}
        class:pointer-events-none={searchStore.isSearching}
      >
        {#if selectedPlaylistData}
          <!-- Playlist View -->
          <DetailHeader
            title={selectedPlaylistData.name}
            subtitle="{selectedPlaylistData.song_count} {selectedPlaylistData.song_count ===
            1
              ? 'song'
              : 'songs'}"
            coverArtId={selectedPlaylistData.cover_art_id}
            onBack={() => handlePlaylistSelect(null)}
          />

          <div class="flex-1 overflow-hidden">
            <SongList
              songs={filteredPlaylistSongs}
              isLoading={playlistStore.isLoading}
              selectedSongId={selectedSong?.id}
              playingSongId={playback.currentTrack?.id ?? null}
              {scrollToSongId}
              playlistId={selectedPlaylist?.id}
              onSelect={handleSongSelect}
              onPlay={handlePlaylistSongPlay}
            />
          </div>

          <StatusBar
            itemCount={filteredPlaylistSongs.length}
            totalDuration={playlistTotalDuration}
            totalSize={playlistTotalSize}
            syncText={syncStatusSummary}
            syncTone={syncStatusTone}
          />
        {:else if activeView === "music"}
          <!-- Music View: Column Browser + Song List -->
          <ColumnBrowser
            genres={filteredGenres}
            artists={filteredArtists}
            albums={filteredAlbums}
            {selectedGenre}
            {selectedArtist}
            {selectedAlbum}
            onGenreSelect={handleGenreSelect}
            onArtistSelect={handleArtistSelect}
            onAlbumSelect={handleAlbumSelect}
            {isLoading}
          />

          <div class="flex-1 overflow-hidden">
            <SongList
              songs={filteredSongs}
              {isLoading}
              error={loadError}
              selectedSongId={selectedSong?.id}
              playingSongId={playback.currentTrack?.id ?? null}
              {scrollToSongId}
              onSelect={handleSongSelect}
              onPlay={handleSongPlay}
            />
          </div>

          <StatusBar
            itemCount={filteredSongs.length}
            {totalDuration}
            {totalSize}
            syncText={syncStatusSummary}
            syncTone={syncStatusTone}
          />
        {:else if activeView === "artists"}
          {#if detailView?.type === "artist" && detailView.artist}
            <!-- Artist Detail: Header + Song List -->
            <DetailHeader
              title={detailView.artist.name}
              subtitle="{detailView.artist.album_count} {detailView.artist
                .album_count === 1
                ? 'album'
                : 'albums'}"
              coverArtId={detailView.artist.cover_art_id}
              onBack={handleDetailBack}
            />

            <div class="flex-1 overflow-hidden">
              <SongList
                songs={detailSongs}
                {isLoading}
                error={loadError}
                selectedSongId={selectedSong?.id}
                playingSongId={playback.currentTrack?.id ?? null}
                {scrollToSongId}
                onSelect={handleSongSelect}
                onPlay={handleDetailSongPlay}
              />
            </div>

            <StatusBar
              itemCount={detailSongs.length}
              totalDuration={detailTotalDuration}
              totalSize={detailTotalSize}
              syncText={syncStatusSummary}
              syncTone={syncStatusTone}
            />
          {:else}
            <!-- Artist Grid -->
            <ArtistGridView
              artists={gridFilteredArtists}
              onSelect={handleArtistGridSelect}
            />

            <StatusBar
              itemCount={gridFilteredArtists.length}
              itemType="artists"
              syncText={syncStatusSummary}
              syncTone={syncStatusTone}
            />
          {/if}
        {:else if activeView === "albums"}
          {#if detailView?.type === "album" && detailView.album}
            <!-- Album Detail: Header + Song List -->
            <DetailHeader
              title={detailView.album.name}
              subtitle={detailView.album.artistName ?? ""}
              coverArtId={detailView.album.cover_art_id}
              onBack={handleDetailBack}
            />

            <div class="flex-1 overflow-hidden">
              <SongList
                songs={detailSongs}
                {isLoading}
                error={loadError}
                selectedSongId={selectedSong?.id}
                playingSongId={playback.currentTrack?.id ?? null}
                {scrollToSongId}
                onSelect={handleSongSelect}
                onPlay={handleDetailSongPlay}
              />
            </div>

            <StatusBar
              itemCount={detailSongs.length}
              totalDuration={detailTotalDuration}
              totalSize={detailTotalSize}
              syncText={syncStatusSummary}
              syncTone={syncStatusTone}
            />
          {:else}
            <!-- Album Grid -->
            <AlbumGridView
              albums={gridFilteredAlbums}
              onSelect={handleAlbumGridSelect}
            />

            <StatusBar
              itemCount={gridFilteredAlbums.length}
              itemType="albums"
              syncText={syncStatusSummary}
              syncTone={syncStatusTone}
            />
          {/if}
        {/if}
      </main>

      <!-- Queue Panel -->
      {#if queueOpen}
        <QueuePanel onItemClick={handleQueueItemClick} />
      {/if}
    </div>
  {:else}
    <!-- Login Screen -->
    <ServerConnect />
  {/if}

  <!-- Settings Modal -->
  <SettingsModal open={settingsOpen} onClose={handleSettingsClose} />
</div>
