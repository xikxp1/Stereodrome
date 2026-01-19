<script lang="ts">
  import type { Artist, Album } from "$lib/types";

  interface Props {
    genres?: string[];
    artists?: Artist[];
    albums?: Album[];
    selectedGenre?: string | null;
    selectedArtist?: Artist | null;
    selectedAlbum?: Album | null;
    onGenreSelect?: (genre: string | null) => void;
    onArtistSelect?: (artist: Artist | null) => void;
    onAlbumSelect?: (album: Album | null) => void;
    isLoading?: boolean;
  }

  let {
    genres = [],
    artists = [],
    albums = [],
    selectedGenre = null,
    selectedArtist = null,
    selectedAlbum = null,
    onGenreSelect,
    onArtistSelect,
    onAlbumSelect,
    isLoading = false,
  }: Props = $props();

  const allGenresCount = $derived(genres.length);
  const allArtistsCount = $derived(artists.length);
  const allAlbumsCount = $derived(albums.length);

  function handleGenreClick(genre: string | null) {
    onGenreSelect?.(genre);
  }

  function handleArtistClick(artist: Artist | null) {
    onArtistSelect?.(artist);
  }

  function handleAlbumClick(album: Album | null) {
    onAlbumSelect?.(album);
  }
</script>

<div class="browser-container h-44 flex select-none">
  <!-- Genres Column -->
  <div class="browser-column flex-1 min-w-0">
    <div class="browser-header">Genres</div>
    <div class="browser-list">
      <button
        class="browser-item w-full text-left"
        class:selected={selectedGenre === null}
        onclick={() => handleGenreClick(null)}
      >
        All ({allGenresCount} Genres)
      </button>
      {#each genres as genre (genre)}
        <button
          class="browser-item w-full text-left"
          class:selected={selectedGenre === genre}
          onclick={() => handleGenreClick(genre)}
        >
          {genre}
        </button>
      {/each}
      {#if genres.length === 0 && !isLoading}
        <div class="browser-item text-base-content/40 cursor-default">
          No genres
        </div>
      {/if}
    </div>
  </div>

  <!-- Artists Column -->
  <div class="browser-column flex-1 min-w-0">
    <div class="browser-header">Artists</div>
    <div class="browser-list">
      <button
        class="browser-item w-full text-left"
        class:selected={selectedArtist === null}
        onclick={() => handleArtistClick(null)}
      >
        All ({allArtistsCount} Artists)
      </button>
      {#each artists as artist (artist.id)}
        <button
          class="browser-item w-full text-left"
          class:selected={selectedArtist?.id === artist.id}
          onclick={() => handleArtistClick(artist)}
        >
          {artist.name}
        </button>
      {/each}
      {#if artists.length === 0 && !isLoading}
        <div class="browser-item text-base-content/40 cursor-default">
          No artists
        </div>
      {/if}
    </div>
  </div>

  <!-- Albums Column -->
  <div class="browser-column flex-1 min-w-0">
    <div class="browser-header">Albums</div>
    <div class="browser-list">
      <button
        class="browser-item w-full text-left"
        class:selected={selectedAlbum === null}
        onclick={() => handleAlbumClick(null)}
      >
        All ({allAlbumsCount} Albums)
      </button>
      {#each albums as album (album.id)}
        <button
          class="browser-item w-full text-left"
          class:selected={selectedAlbum?.id === album.id}
          onclick={() => handleAlbumClick(album)}
        >
          {album.name}
        </button>
      {/each}
      {#if albums.length === 0 && !isLoading}
        <div class="browser-item text-base-content/40 cursor-default">
          No albums
        </div>
      {/if}
    </div>
  </div>
</div>
