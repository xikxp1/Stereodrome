<script lang="ts">
  import ServerConnect from "$lib/components/ServerConnect.svelte";
  import TransportBar from "$lib/components/TransportBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ColumnBrowser from "$lib/components/library/ColumnBrowser.svelte";
  import SongList from "$lib/components/library/SongList.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import { connection } from "$lib/stores/connection.svelte";
  import { getArtists, getAlbums, getSongs } from "$lib/api/commands";
  import type { Artist, Album, Song } from "$lib/types";

  // View state
  let activeView = $state("music");

  // Browser state
  let genres = $state<string[]>([]);
  let artists = $state<Artist[]>([]);
  let albums = $state<Album[]>([]);
  let songs = $state<Song[]>([]);

  let selectedGenre = $state<string | null>(null);
  let selectedArtist = $state<Artist | null>(null);
  let selectedAlbum = $state<Album | null>(null);
  let selectedSong = $state<Song | null>(null);

  // Playback state (placeholder)
  let isPlaying = $state(false);
  let currentTrack = $state<{ title: string; artist: string } | null>(null);
  let currentTime = $state(0);
  let duration = $state(0);
  let volume = $state(80);

  // Loading states
  let isLoading = $state(false);
  let loadError = $state<Error | null>(null);

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

      // Extract unique genres
      genres = [
        ...new Set(songsData.map((s) => s.genre).filter(Boolean) as string[]),
      ].sort();
    } catch (e) {
      loadError = e instanceof Error ? e : new Error(String(e));
    } finally {
      isLoading = false;
    }
  }

  // Filtered data based on selections
  const filteredArtists = $derived(() => {
    if (!selectedGenre) return artists;
    const artistIds = new Set(
      songs.filter((s) => s.genre === selectedGenre).map((s) => s.artist_id)
    );
    return artists.filter((a) => artistIds.has(a.id));
  });

  const filteredAlbums = $derived(() => {
    let result = albums;
    if (selectedGenre) {
      const albumIds = new Set(
        songs.filter((s) => s.genre === selectedGenre).map((s) => s.album_id)
      );
      result = result.filter((a) => albumIds.has(a.id));
    }
    if (selectedArtist) {
      result = result.filter((a) => a.artist_id === selectedArtist.id);
    }
    return result;
  });

  const filteredSongs = $derived(() => {
    let result = songs;
    if (selectedGenre) {
      result = result.filter((s) => s.genre === selectedGenre);
    }
    if (selectedArtist) {
      result = result.filter((s) => s.artist_id === selectedArtist.id);
    }
    if (selectedAlbum) {
      result = result.filter((s) => s.album_id === selectedAlbum.id);
    }
    return result;
  });

  // Stats for status bar
  const totalDuration = $derived(
    filteredSongs().reduce((acc, s) => acc + (s.duration || 0), 0)
  );
  const totalSize = $derived(
    filteredSongs().reduce((acc, s) => acc + (s.size || 0), 0)
  );

  // Handlers
  function handleGenreSelect(genre: string | null) {
    selectedGenre = genre;
    selectedArtist = null;
    selectedAlbum = null;
  }

  function handleArtistSelect(artist: Artist | null) {
    selectedArtist = artist;
    selectedAlbum = null;
  }

  function handleAlbumSelect(album: Album | null) {
    selectedAlbum = album;
  }

  function handleSongSelect(song: Song) {
    selectedSong = song;
  }

  function handleSongPlay(song: Song) {
    currentTrack = {
      title: song.title,
      artist: song.artist || "Unknown Artist",
    };
    isPlaying = true;
    duration = song.duration || 0;
    currentTime = 0;
    // Actual playback would be implemented here
  }

  function handlePlayPause() {
    isPlaying = !isPlaying;
  }

  function handleViewChange(view: string) {
    activeView = view;
  }
</script>

<div class="h-screen flex flex-col bg-base-200 overflow-hidden">
  {#if connection.status.connected}
    <!-- Transport Bar -->
    <TransportBar
      {isPlaying}
      {currentTrack}
      {currentTime}
      {duration}
      {volume}
      onPlayPause={handlePlayPause}
      onVolumeChange={(v) => (volume = v)}
    />

    <!-- Main Content Area -->
    <div class="flex-1 flex overflow-hidden">
      <!-- Sidebar -->
      <aside class="w-48 flex-shrink-0">
        <Sidebar {activeView} onViewChange={handleViewChange} />
      </aside>

      <!-- Content -->
      <main
        class="flex-1 flex flex-col overflow-hidden border-l border-base-300"
      >
        <!-- Column Browser -->
        <ColumnBrowser
          {genres}
          artists={filteredArtists()}
          albums={filteredAlbums()}
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
            songs={filteredSongs()}
            {isLoading}
            error={loadError}
            selectedSongId={selectedSong?.id}
            playingSongId={currentTrack
              ? songs.find((s) => s.title === currentTrack.title)?.id
              : null}
            onSelect={handleSongSelect}
            onPlay={handleSongPlay}
          />
        </div>

        <!-- Status Bar -->
        <StatusBar
          itemCount={filteredSongs().length}
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
