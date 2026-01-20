<script lang="ts">
  import ServerConnect from "$lib/components/ServerConnect.svelte";
  import TransportBar from "$lib/components/TransportBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ColumnBrowser from "$lib/components/library/ColumnBrowser.svelte";
  import SongList from "$lib/components/library/SongList.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import { connection } from "$lib/stores/connection.svelte";
  import { playback } from "$lib/stores/playback.svelte";
  import { queue } from "$lib/stores/queue.svelte";
  import { searchStore } from "$lib/stores/search.svelte";
  import { getArtists, getAlbums, getSongs } from "$lib/api/commands";
  import type { Artist, Album, Song } from "$lib/types";

  // View state
  let activeView = $state("music");

  // Browser state
  let artists = $state<Artist[]>([]);
  let albums = $state<Album[]>([]);
  let songs = $state<Song[]>([]);

  let selectedGenre = $state<string | null>(null);
  let selectedArtist = $state<Artist | null>(null);
  let selectedAlbum = $state<Album | null>(null);
  let selectedSong = $state<Song | null>(null);

  // Get current track from local playback state (no server latency)
  const currentTrack = $derived(playback.currentTrack);
  const volume = $derived(playback.volume * 100); // Convert to 0-100 for UI

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

    // Single pass through songs
    const matchedGenres = new Set<string>();
    const matchedArtistIds = new Set<string>();
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

  function handlePrevious() {
    queue.playPrevious();
  }

  function handleNext() {
    queue.playNext();
  }

  function handleViewChange(view: string) {
    activeView = view;
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
      onPlayPause={handlePlayPause}
      onPrevious={handlePrevious}
      onNext={handleNext}
      onVolumeChange={handleVolumeChange}
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
            playingSongId={playback.currentSong?.id ?? null}
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
    </div>
  {:else}
    <!-- Login Screen -->
    <ServerConnect />
  {/if}
</div>
