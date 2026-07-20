<script lang="ts">
  import AuthLoadingScreen from "$lib/components/AuthLoadingScreen.svelte";
  import ServerConnect from "$lib/components/ServerConnect.svelte";
  import TransportBar from "$lib/components/TransportBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ColumnBrowser from "$lib/components/library/ColumnBrowser.svelte";
  import SongList from "$lib/components/library/SongList.svelte";
  import ArtistGridView from "$lib/components/library/ArtistGridView.svelte";
  import AlbumGridView from "$lib/components/library/AlbumGridView.svelte";
  import ArtistAlbumRail from "$lib/components/library/ArtistAlbumRail.svelte";
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
    seekPlayback,
    getCoverArt,
    getMiniPlayerPosition,
    getOfflineSongIds,
    getDownloadingSongIds,
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
  import { on } from "svelte/events";
  import { CircleCheck, Download } from "lucide-svelte";
  import { playlistStore } from "$lib/stores/playlist.svelte";
  import { albumListStore } from "$lib/stores/albumList.svelte";
  import { LIBRARY_REFRESHED_EVENT } from "$lib/services/libraryRefresh.svelte";
  import type {
    Artist,
    Album,
    AlbumListEntry,
    Song,
    Playlist,
    MiniPlayerPosition,
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
  let albumDetailBackArtist: Artist | null = $state(null);

  // Scroll restoration state
  let artistGridScrollOffset: number | null = $state(null);
  let albumGridScrollOffset: number | null = $state(null);

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
  const authBootstrapPending = $derived(
    connection.isInitializing || !connection.hasInitialized
  );
  const hasConfiguredServer = $derived(Boolean(connection.status.server_url));
  const isOfflineConfiguredSession = $derived(connection.offlineMode);
  const configuredAccountKey = $derived.by(() => {
    const serverUrl = connection.status.server_url;
    if (!serverUrl) return null;
    return `${serverUrl}::${connection.status.username ?? ""}`;
  });

  // Cover art state
  let coverArtUrl = $state<string | null>(null);
  let lastCoverArtId = $state<string | null>(null);
  let loadedLibraryAccountKey = $state<string | null>(null);
  let offlineSongIds = $state<Set<string>>(new Set());
  let downloadingSongIds = $state<Set<string>>(new Set());
  let offlineSongIdsRefreshTimeout: ReturnType<typeof setTimeout> | null = null;

  const MINI_PLAYER_WIDTH = 320;
  const MINI_PLAYER_HEIGHT = 72;
  const MINI_PLAYER_MARGIN = 8;
  const OFFLINE_SONG_IDS_REFRESH_DEBOUNCE_MS = 200;

  interface AudioCacheChangedEvent {
    reason: string;
  }

  interface LogicalMonitorBounds {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  interface CoverArtWindowData {
    id: string;
    album: string;
    artist: string;
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

  // Check for updates on startup (runs once after connection)
  let updateChecked = false;
  $effect(() => {
    if (
      connection.status.connected &&
      !connection.manualOfflineEnabled &&
      !updateChecked
    ) {
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
    return on(window, "open-settings", handler);
  });

  $effect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | null = null;

    listen<AudioCacheChangedEvent>("audio-cache-changed", () => {
      scheduleOfflineSongIdsRefresh();
    })
      .then((listener) => {
        if (disposed) {
          listener();
          return;
        }
        unlisten = listener;
      })
      .catch((e) => {
        error(`Failed to listen for audio cache changes: ${e}`);
      });

    return () => {
      disposed = true;
      unlisten?.();
      if (offlineSongIdsRefreshTimeout) {
        clearTimeout(offlineSongIdsRefreshTimeout);
        offlineSongIdsRefreshTimeout = null;
      }
    };
  });

  $effect(() => {
    const handler = () => {
      if (!connection.status.server_url) return;
      void loadLibraryData();
      if (
        connection.status.connected &&
        !connection.manualOfflineEnabled &&
        (activeView === "recently_added" ||
          activeView === "recently_played" ||
          activeView === "most_played")
      ) {
        void albumListStore.loadView(activeView, { force: true });
      }
    };

    return on(window, LIBRARY_REFRESHED_EVENT, handler);
  });

  // Load local library data for configured sessions, including offline restores.
  $effect(() => {
    const accountKey = configuredAccountKey;

    if (!accountKey) {
      loadedLibraryAccountKey = null;
      artists = [];
      albums = [];
      songs = [];
      offlineSongIds = new Set();
      downloadingSongIds = new Set();
      selectedGenre = null;
      selectedArtist = null;
      selectedAlbum = null;
      selectedSong = null;
      detailView = null;
      albumDetailBackArtist = null;
      loadError = null;
      isLoading = false;
      return;
    }

    if (loadedLibraryAccountKey !== accountKey) {
      loadedLibraryAccountKey = accountKey;
      void loadLibraryData();
    }
  });

  // Fetch album list data when switching to album list views
  $effect(() => {
    if (
      connection.status.connected &&
      !connection.manualOfflineEnabled &&
      (activeView === "recently_added" ||
        activeView === "recently_played" ||
        activeView === "most_played")
    ) {
      void albumListStore.loadView(activeView);
    }
  });

  async function loadLibraryData() {
    isLoading = true;
    loadError = null;
    try {
      const [
        artistsData,
        albumsData,
        songsData,
        offlineSongIdData,
        downloadingSongIdData,
      ] = await Promise.all([
        getArtists(),
        getAlbums(),
        getSongs(),
        getOfflineSongIds(),
        getDownloadingSongIds(),
      ]);
      artists = artistsData;
      albums = albumsData;
      songs = songsData;
      offlineSongIds = new Set(offlineSongIdData);
      downloadingSongIds = new Set(downloadingSongIdData);
      if (connection.status.connected && !connection.manualOfflineEnabled) {
        void playlistStore
          .reconcileSavedPlaylistsOffline()
          .then(() => refreshOfflineSongIds());
      }
    } catch (e) {
      loadError = e instanceof Error ? e : new Error(String(e));
    } finally {
      isLoading = false;
    }
  }

  function scheduleOfflineSongIdsRefresh() {
    if (!connection.status.server_url) return;

    if (offlineSongIdsRefreshTimeout) {
      clearTimeout(offlineSongIdsRefreshTimeout);
    }

    offlineSongIdsRefreshTimeout = setTimeout(() => {
      offlineSongIdsRefreshTimeout = null;
      void refreshOfflineSongIds();
    }, OFFLINE_SONG_IDS_REFRESH_DEBOUNCE_MS);
  }

  async function refreshOfflineSongIds() {
    const accountKey = configuredAccountKey;
    if (!accountKey) {
      offlineSongIds = new Set();
      downloadingSongIds = new Set();
      return;
    }

    try {
      const [offlineSongIdData, downloadingSongIdData] = await Promise.all([
        getOfflineSongIds(),
        getDownloadingSongIds(),
      ]);
      if (configuredAccountKey === accountKey) {
        offlineSongIds = new Set(offlineSongIdData);
        downloadingSongIds = new Set(downloadingSongIdData);
      }
    } catch (e) {
      error(`Failed to refresh offline song IDs: ${e}`);
    }
  }

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

  const offlineVisibleSongs = $derived(
    isOfflineConfiguredSession
      ? songs.filter((song) => offlineSongIds.has(song.id))
      : songs
  );

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

    for (const s of offlineVisibleSongs) {
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
  const gridFilteredArtists = $derived.by(() => {
    const visibleArtistIds = new Set(
      offlineVisibleSongs.map((song) => song.artist_id)
    );
    const baseArtists = isOfflineConfiguredSession
      ? artists.filter((artist) => visibleArtistIds.has(artist.id))
      : artists;

    return searchStore.hasActiveQuery
      ? baseArtists.filter((artist) =>
          searchStore.matchedArtistIds.has(artist.id)
        )
      : baseArtists;
  });
  const gridFilteredAlbums = $derived.by(() => {
    const visibleAlbumIds = new Set(
      offlineVisibleSongs.map((song) => song.album_id)
    );
    const baseAlbums = isOfflineConfiguredSession
      ? albums.filter((album) => visibleAlbumIds.has(album.id))
      : albums;

    return searchStore.hasActiveQuery
      ? baseAlbums.filter((album) => searchStore.matchedAlbumIds.has(album.id))
      : baseAlbums;
  });

  // Songs for detail view
  const detailSongs = $derived.by(() => {
    const detail = detailView;
    if (!detail) return [];
    if (detail.type === "artist" && detail.artist) {
      const artistId = detail.artist.id;
      return offlineVisibleSongs.filter((s) => s.artist_id === artistId);
    }
    if (detail.type === "album" && detail.album) {
      const albumId = detail.album.id;
      return offlineVisibleSongs.filter((s) => s.album_id === albumId);
    }
    return [];
  });

  function compareArtistAlbums(left: Album, right: Album) {
    const leftYear = left.year ?? Number.NEGATIVE_INFINITY;
    const rightYear = right.year ?? Number.NEGATIVE_INFINITY;

    if (leftYear !== rightYear) {
      return leftYear - rightYear;
    }

    const nameSort = left.name.localeCompare(right.name);
    return nameSort !== 0 ? nameSort : left.id.localeCompare(right.id);
  }

  const detailArtistAlbums = $derived.by(() => {
    const detail = detailView;
    if (!detail || detail.type !== "artist" || !detail.artist) return [];

    const artistId = detail.artist.id;
    const visibleAlbumIds = new Set(detailSongs.map((song) => song.album_id));

    return albums
      .filter(
        (album) => album.artist_id === artistId && visibleAlbumIds.has(album.id)
      )
      .sort(compareArtistAlbums);
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
  const offlineVisiblePlaylistSongs = $derived(
    isOfflineConfiguredSession
      ? playlistSongs.filter((song) => offlineSongIds.has(song.id))
      : playlistSongs
  );
  const filteredPlaylistSongs = $derived(
    searchStore.hasActiveQuery
      ? offlineVisiblePlaylistSongs.filter((s) =>
          searchStore.matchedSongIds.has(s.id)
        )
      : offlineVisiblePlaylistSongs
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
    albumDetailBackArtist = null;
    selectedPlaylist = null; // Deselect playlist when changing library views
    artistGridScrollOffset = null;
    albumGridScrollOffset = null;
  }

  function handlePlaylistSelect(playlist: Playlist | null) {
    selectedPlaylist = playlist;
    if (playlist) {
      playlistStore.selectPlaylist(playlist);
      detailView = null;
      albumDetailBackArtist = null;
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

  async function handlePlaylistSavedOfflineToggle(playlist: Playlist) {
    const updated = await playlistStore.setPlaylistSavedOffline(
      playlist.id,
      !playlist.saved_offline
    );
    if (updated) {
      await refreshOfflineSongIds();
    }
  }

  function handleArtistGridSelect(artist: Artist) {
    detailView = { type: "artist", artist };
    albumDetailBackArtist = null;
  }

  function handleAlbumGridSelect(album: Album | AlbumListEntry) {
    albumDetailBackArtist = null;
    if ("synced_at" in album) {
      // Full Album from local cache
      detailView = { type: "album", album: album as Album };
    } else {
      // AlbumListEntry from server
      handleAlbumListEntrySelect(album as AlbumListEntry);
    }
  }

  function handleAlbumListEntrySelect(entry: Album | AlbumListEntry) {
    albumDetailBackArtist = null;
    // Try to find in local cache for full data
    const localAlbum = albums.find((a) => a.id === entry.id);
    if (localAlbum) {
      detailView = { type: "album", album: localAlbum };
    } else {
      // Create a minimal Album from the entry
      detailView = {
        type: "album",
        album: {
          id: entry.id,
          artist_id: entry.artist_id ?? "",
          name: entry.name,
          year: entry.year,
          song_count: entry.song_count ?? 0,
          duration: entry.duration,
          cover_art_id: entry.cover_art_id,
          synced_at: "",
          ...(entry.artistName ? { artistName: entry.artistName } : {}),
        },
      };
    }
  }

  function handleAlbumListEntryNavigateToArtist(album: Album | AlbumListEntry) {
    const artistId = album.artist_id;
    if (artistId) {
      navigateToArtist(artistId);
    }
  }

  function navigateToArtist(artistId: string) {
    const artist = artists.find((entry) => entry.id === artistId);
    if (!artist) return;

    activeView = "artists";
    selectedPlaylist = null;
    selectedArtist = artist;
    selectedAlbum = null;
    detailView = { type: "artist", artist };
    albumDetailBackArtist = null;
  }

  function navigateToAlbum(albumId: string, backArtist: Artist | null = null) {
    const album = albums.find((entry) => entry.id === albumId);
    if (!album) return;

    activeView = "albums";
    selectedPlaylist = null;
    selectedAlbum = album;
    selectedArtist =
      artists.find((entry) => entry.id === album.artist_id) ?? null;
    detailView = { type: "album", album };
    albumDetailBackArtist = backArtist;
  }

  function handleDetailBack() {
    if (detailView?.type === "album" && albumDetailBackArtist) {
      const artist = albumDetailBackArtist;
      activeView = "artists";
      selectedArtist = artist;
      selectedAlbum = null;
      detailView = { type: "artist", artist };
      albumDetailBackArtist = null;
      return;
    }

    detailView = null;
    albumDetailBackArtist = null;
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

  function handleSongNavigateToArtist(song: Song) {
    navigateToArtist(song.artist_id);
  }

  function handleSongNavigateToAlbum(song: Song) {
    navigateToAlbum(song.album_id);
  }

  function handleArtistAlbumSelect(album: Album) {
    const backArtist =
      detailView?.type === "artist" ? (detailView.artist ?? null) : null;
    navigateToAlbum(album.id, backArtist);
  }

  function handleAlbumNavigateToArtist(album: Album | AlbumListEntry) {
    if (album.artist_id) {
      navigateToArtist(album.artist_id);
    }
  }

  async function openCoverArtWindow(coverArtData: CoverArtWindowData) {
    const title = `${coverArtData.artist}${coverArtData.album ? ` — ${coverArtData.album}` : ""}`;
    // Check if window already exists
    const existingWindow = await WebviewWindow.getByLabel("cover-art-viewer");
    if (existingWindow) {
      // Update existing window with new cover art
      await emit("cover-art-update", coverArtData);
      await existingWindow.setTitle(title);
      await existingWindow.setFocus();
      return;
    }

    // Create a new window for cover art viewing
    const webview = new WebviewWindow("cover-art-viewer", {
      url: `/cover-art?id=${encodeURIComponent(coverArtData.id)}&album=${encodeURIComponent(coverArtData.album)}&artist=${encodeURIComponent(coverArtData.artist)}`,
      title,
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

  async function handleCoverArtClick() {
    if (!currentTrack?.coverArtId) return;

    try {
      await openCoverArtWindow({
        id: currentTrack.coverArtId,
        album: currentTrack.album,
        artist: currentTrack.artist,
      });
    } catch (e) {
      error(`Failed to open cover art window: ${e}`);
    }
  }

  function getAlbumArtistName(album: Album | AlbumListEntry): string {
    return (
      album.artistName ??
      (album.artist_id
        ? (artists.find((artist) => artist.id === album.artist_id)?.name ??
          "Unknown Artist")
        : "Unknown Artist")
    );
  }

  async function handleAlbumCoverArtClick(album: Album | AlbumListEntry) {
    if (!album.cover_art_id) return;

    try {
      await openCoverArtWindow({
        id: album.cover_art_id,
        album: album.name,
        artist: getAlbumArtistName(album),
      });
    } catch (e) {
      error(`Failed to open album cover art window: ${e}`);
    }
  }

  function handleDetailAlbumCoverArtClick() {
    const detail = detailView;
    if (detail?.type !== "album" || !detail.album) return;

    void handleAlbumCoverArtClick(detail.album);
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
  {#if authBootstrapPending}
    <AuthLoadingScreen />
  {:else if hasConfiguredServer}
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
          selectedPlaylistId={selectedPlaylist?.id ?? null}
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
            actionLabel={selectedPlaylistData.saved_offline
              ? "Saved Offline"
              : "Save Offline"}
            actionTitle={selectedPlaylistData.saved_offline
              ? "Remove playlist from offline listening"
              : "Save playlist for offline listening"}
            actionIcon={selectedPlaylistData.saved_offline
              ? CircleCheck
              : Download}
            onAction={() =>
              handlePlaylistSavedOfflineToggle(selectedPlaylistData)}
          />

          <div class="flex-1 overflow-hidden">
            <SongList
              songs={filteredPlaylistSongs}
              isLoading={playlistStore.isLoading}
              selectedSongId={selectedSong?.id ?? null}
              playingSongId={playback.currentTrack?.id ?? null}
              {scrollToSongId}
              playlistId={selectedPlaylist?.id ?? null}
              downloadedSongIds={offlineSongIds}
              {downloadingSongIds}
              onSelect={handleSongSelect}
              onPlay={handlePlaylistSongPlay}
              onNavigateToArtist={handleSongNavigateToArtist}
              onNavigateToAlbum={handleSongNavigateToAlbum}
            />
          </div>

          <StatusBar
            itemCount={filteredPlaylistSongs.length}
            totalDuration={playlistTotalDuration}
            totalSize={playlistTotalSize}
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
              selectedSongId={selectedSong?.id ?? null}
              playingSongId={playback.currentTrack?.id ?? null}
              {scrollToSongId}
              downloadedSongIds={offlineSongIds}
              {downloadingSongIds}
              onSelect={handleSongSelect}
              onPlay={handleSongPlay}
              onNavigateToArtist={handleSongNavigateToArtist}
              onNavigateToAlbum={handleSongNavigateToAlbum}
            />
          </div>

          <StatusBar
            itemCount={filteredSongs.length}
            {totalDuration}
            {totalSize}
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

            <ArtistAlbumRail
              albums={detailArtistAlbums}
              onSelect={handleArtistAlbumSelect}
            />

            <div class="flex-1 overflow-hidden">
              <SongList
                songs={detailSongs}
                {isLoading}
                error={loadError}
                selectedSongId={selectedSong?.id ?? null}
                playingSongId={playback.currentTrack?.id ?? null}
                {scrollToSongId}
                downloadedSongIds={offlineSongIds}
                {downloadingSongIds}
                onSelect={handleSongSelect}
                onPlay={handleDetailSongPlay}
                onNavigateToArtist={handleSongNavigateToArtist}
                onNavigateToAlbum={handleSongNavigateToAlbum}
              />
            </div>

            <StatusBar
              itemCount={detailSongs.length}
              totalDuration={detailTotalDuration}
              totalSize={detailTotalSize}
            />
          {:else}
            <!-- Artist Grid -->
            <ArtistGridView
              artists={gridFilteredArtists}
              onSelect={handleArtistGridSelect}
              restoreScrollOffset={artistGridScrollOffset}
              onScrollOffsetChange={(offset) => {
                artistGridScrollOffset = offset;
              }}
            />

            <StatusBar
              itemCount={gridFilteredArtists.length}
              itemType="artists"
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
              onCoverArtClick={handleDetailAlbumCoverArtClick}
            />

            <div class="flex-1 overflow-hidden">
              <SongList
                songs={detailSongs}
                {isLoading}
                error={loadError}
                selectedSongId={selectedSong?.id ?? null}
                playingSongId={playback.currentTrack?.id ?? null}
                {scrollToSongId}
                downloadedSongIds={offlineSongIds}
                {downloadingSongIds}
                onSelect={handleSongSelect}
                onPlay={handleDetailSongPlay}
                onNavigateToArtist={handleSongNavigateToArtist}
                onNavigateToAlbum={handleSongNavigateToAlbum}
              />
            </div>

            <StatusBar
              itemCount={detailSongs.length}
              totalDuration={detailTotalDuration}
              totalSize={detailTotalSize}
            />
          {:else}
            <!-- Album Grid -->
            <AlbumGridView
              albums={gridFilteredAlbums}
              onSelect={handleAlbumGridSelect}
              onNavigateToArtist={handleAlbumNavigateToArtist}
              restoreScrollOffset={albumGridScrollOffset}
              onScrollOffsetChange={(offset) => {
                albumGridScrollOffset = offset;
              }}
            />

            <StatusBar
              itemCount={gridFilteredAlbums.length}
              itemType="albums"
            />
          {/if}
        {:else if activeView === "recently_added" || activeView === "recently_played" || activeView === "most_played"}
          {#if detailView?.type === "album" && detailView.album}
            <!-- Album Detail: Header + Song List -->
            <DetailHeader
              title={detailView.album.name}
              subtitle={detailView.album.artistName ?? ""}
              coverArtId={detailView.album.cover_art_id}
              onBack={handleDetailBack}
              onCoverArtClick={handleDetailAlbumCoverArtClick}
            />

            <div class="flex-1 overflow-hidden">
              <SongList
                songs={detailSongs}
                isLoading={albumListStore.isLoading}
                error={albumListStore.error}
                selectedSongId={selectedSong?.id ?? null}
                playingSongId={playback.currentTrack?.id ?? null}
                {scrollToSongId}
                downloadedSongIds={offlineSongIds}
                {downloadingSongIds}
                onSelect={handleSongSelect}
                onPlay={handleDetailSongPlay}
                onNavigateToArtist={handleSongNavigateToArtist}
                onNavigateToAlbum={handleSongNavigateToAlbum}
              />
            </div>

            <StatusBar
              itemCount={detailSongs.length}
              totalDuration={detailTotalDuration}
              totalSize={detailTotalSize}
            />
          {:else}
            <!-- Album List Grid -->
            <AlbumGridView
              albums={albumListStore.entries}
              totalCount={albumListStore.totalCount}
              onSelect={handleAlbumListEntrySelect}
              onNavigateToArtist={handleAlbumListEntryNavigateToArtist}
              restoreScrollOffset={albumGridScrollOffset}
              onScrollOffsetChange={(offset) => {
                albumGridScrollOffset = offset;
              }}
            />

            <StatusBar
              itemCount={albumListStore.entries.length}
              totalCount={albumListStore.totalCount}
              itemType="albums"
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
