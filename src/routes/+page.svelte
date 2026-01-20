<script lang="ts">
  import ServerConnect from "$lib/components/ServerConnect.svelte";
  import TransportBar from "$lib/components/TransportBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ColumnBrowser from "$lib/components/library/ColumnBrowser.svelte";
  import SongList from "$lib/components/library/SongList.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import QueuePanel from "$lib/components/QueuePanel.svelte";
  import { connection } from "$lib/stores/connection.svelte";
  import { playback } from "$lib/stores/playback.svelte";
  import { queue } from "$lib/stores/queue.svelte";
  import { searchStore } from "$lib/stores/search.svelte";
  import {
    getArtists,
    getAlbums,
    getSongs,
    seekPlayback,
    getCoverArt,
  } from "$lib/api/commands";
  import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { emit } from "@tauri-apps/api/event";
  import type { Artist, Album, Song } from "$lib/types";

  // View state
  let activeView = $state("music");
  let queueOpen = $state(false);

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
  const volume = $derived(playback.volume * 100); // Convert to 0-100 for UI

  // Cover art state
  let coverArtUrl = $state<string | null>(null);
  let lastCoverArtId = $state<string | null>(null);

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
          console.error("Failed to fetch cover art thumbnail:", e);
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

  // Restore session on mount (runs once)
  let sessionRestored = false;
  $effect(() => {
    if (!sessionRestored) {
      sessionRestored = true;
      connection.restore();
    }
  });

  // Load data when connected
  $effect(() => {
    if (connection.status.connected) {
      loadLibraryData();
    }
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
      console.error("Failed to play song:", e);
    }
  }

  function handlePlayPause() {
    playback.togglePlayPause();
  }

  function handleVolumeChange(v: number) {
    playback.setVolume(v / 100); // Convert from 0-100 to 0-1
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

  function handleViewChange(view: string) {
    activeView = view;
  }

  function handleQueueToggle() {
    queueOpen = !queueOpen;
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
      console.error("Failed to create cover art window:", e);
    });
  }
</script>

<div class="h-screen flex flex-col bg-base-200 overflow-hidden">
  {#if connection.status.connected}
    <!-- Transport Bar -->
    <TransportBar
      isPlaying={playback.isPlaying}
      {currentTrack}
      currentTime={playback.position}
      duration={playback.duration}
      {volume}
      {queueOpen}
      {coverArtUrl}
      onPlayPause={handlePlayPause}
      onPrevious={handlePrevious}
      onNext={handleNext}
      onSeek={handleSeek}
      onVolumeChange={handleVolumeChange}
      onQueueToggle={handleQueueToggle}
      onCoverArtClick={handleCoverArtClick}
    />

    <!-- Main Content Area -->
    <div class="flex-1 flex overflow-hidden">
      <!-- Sidebar -->
      <aside class="w-48 flex-shrink-0">
        <Sidebar
          {activeView}
          onViewChange={handleViewChange}
          onSync={loadLibraryData}
        />
      </aside>

      <!-- Content -->
      <main
        class="flex-1 flex flex-col overflow-hidden border-l border-base-300 transition-opacity duration-150"
        class:opacity-50={searchStore.isSearching}
        class:pointer-events-none={searchStore.isSearching}
      >
        <!-- Column Browser -->
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

        <!-- Song List -->
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

        <!-- Status Bar -->
        <StatusBar
          itemCount={filteredSongs.length}
          {totalDuration}
          {totalSize}
        />
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
</div>
