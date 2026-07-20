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

<div class="h-44 flex select-none bg-base-100 border-b border-base-300">
  <!-- Genres Column -->
  <div
    class="flex-1 min-w-0 flex flex-col border-r border-base-300 overflow-hidden"
  >
    <div
      class="text-xs font-semibold text-center py-1.5 bg-base-200 border-b border-base-300 shrink-0"
    >
      Genres
    </div>
    <div class="flex-1 overflow-y-auto">
      <div class="flex flex-col">
        <button
          type="button"
          class="px-2 py-0.5 text-xs text-left truncate {selectedGenre === null
            ? 'bg-primary text-primary-content'
            : 'hover:bg-base-200'}"
          onclick={() => handleGenreClick(null)}
        >
          All ({allGenresCount} Genres)
        </button>
        {#each genres as genre (genre)}
          <button
            type="button"
            class="px-2 py-0.5 text-xs text-left truncate {selectedGenre ===
            genre
              ? 'bg-primary text-primary-content'
              : 'hover:bg-base-200'}"
            onclick={() => handleGenreClick(genre)}
          >
            {genre}
          </button>
        {/each}
        {#if genres.length === 0 && !isLoading}
          <span class="px-2 py-0.5 text-xs opacity-40">No genres</span>
        {/if}
      </div>
    </div>
  </div>

  <!-- Artists Column -->
  <div
    class="flex-1 min-w-0 flex flex-col border-r border-base-300 overflow-hidden"
  >
    <div
      class="text-xs font-semibold text-center py-1.5 bg-base-200 border-b border-base-300 shrink-0"
    >
      Artists
    </div>
    <div class="flex-1 overflow-y-auto">
      <div class="flex flex-col">
        <button
          type="button"
          class="px-2 py-0.5 text-xs text-left truncate {selectedArtist === null
            ? 'bg-primary text-primary-content'
            : 'hover:bg-base-200'}"
          onclick={() => handleArtistClick(null)}
        >
          All ({allArtistsCount} Artists)
        </button>
        {#each artists as artist (artist.id)}
          <button
            type="button"
            class="px-2 py-0.5 text-xs text-left truncate {selectedArtist?.id ===
            artist.id
              ? 'bg-primary text-primary-content'
              : 'hover:bg-base-200'}"
            onclick={() => handleArtistClick(artist)}
          >
            {artist.name}
          </button>
        {/each}
        {#if artists.length === 0 && !isLoading}
          <span class="px-2 py-0.5 text-xs opacity-40">No artists</span>
        {/if}
      </div>
    </div>
  </div>

  <!-- Albums Column -->
  <div class="flex-1 min-w-0 flex flex-col overflow-hidden">
    <div
      class="text-xs font-semibold text-center py-1.5 bg-base-200 border-b border-base-300 shrink-0"
    >
      Albums
    </div>
    <div class="flex-1 overflow-y-auto">
      <div class="flex flex-col">
        <button
          type="button"
          class="px-2 py-0.5 text-xs text-left truncate {selectedAlbum === null
            ? 'bg-primary text-primary-content'
            : 'hover:bg-base-200'}"
          onclick={() => handleAlbumClick(null)}
        >
          All ({allAlbumsCount} Albums)
        </button>
        {#each albums as album (album.id)}
          <button
            type="button"
            class="px-2 py-0.5 text-xs text-left truncate {selectedAlbum?.id ===
            album.id
              ? 'bg-primary text-primary-content'
              : 'hover:bg-base-200'}"
            onclick={() => handleAlbumClick(album)}
          >
            {album.name}
          </button>
        {/each}
        {#if albums.length === 0 && !isLoading}
          <span class="px-2 py-0.5 text-xs opacity-40">No albums</span>
        {/if}
      </div>
    </div>
  </div>
</div>
