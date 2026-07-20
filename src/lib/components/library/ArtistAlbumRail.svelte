<script lang="ts">
  import LazyImage from "$lib/components/LazyImage.svelte";
  import type { Album } from "$lib/types";

  interface Props {
    albums: Album[];
    onSelect?: (album: Album) => void;
  }

  let { albums, onSelect }: Props = $props();

  function formatAlbumMeta(album: Album): string {
    const parts = [];

    if (album.year) {
      parts.push(String(album.year));
    }

    parts.push(
      `${album.song_count} ${album.song_count === 1 ? "song" : "songs"}`
    );

    return parts.join(" · ");
  }
</script>

{#if albums.length > 0}
  <section class="album-rail border-b border-base-300 bg-base-100">
    <div class="flex items-center justify-between px-4 pt-3">
      <h3 class="text-xs font-semibold uppercase text-base-content/50">
        Albums
      </h3>
      <span class="text-xs text-base-content/45 tabular-nums">
        {albums.length}
      </span>
    </div>

    <div class="album-scroll flex gap-3 overflow-x-auto px-4 pb-3 pt-2">
      {#each albums as album (album.id)}
        <button
          type="button"
          class="album-card group w-28 shrink-0 rounded-md p-2 text-left transition-colors hover:bg-base-200 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
          title={album.name}
          onclick={() => onSelect?.(album)}
        >
          <LazyImage
            coverArtId={album.cover_art_id}
            size={160}
            alt={album.name}
            class="mb-2 h-24 w-24 rounded-md"
          />
          <span
            class="block truncate text-xs font-medium text-base-content group-hover:text-base-content"
          >
            {album.name}
          </span>
          <span class="block truncate text-xs text-base-content/55">
            {formatAlbumMeta(album)}
          </span>
        </button>
      {/each}
    </div>
  </section>
{/if}

<style>
  .album-rail {
    flex-shrink: 0;
  }

  .album-scroll {
    scrollbar-width: thin;
  }

  .album-card {
    min-width: 7rem;
  }
</style>
