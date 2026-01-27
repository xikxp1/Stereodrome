<script lang="ts">
  import type { Album } from "$lib/types";
  import LazyImage from "$lib/components/LazyImage.svelte";

  interface Props {
    albums: Album[];
    onSelect?: (album: Album) => void;
  }

  let { albums, onSelect }: Props = $props();
</script>

<div class="flex-1 overflow-auto p-4">
  {#if albums.length > 0}
    <div
      class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-4"
    >
      {#each albums as album (album.id)}
        <button
          class="flex flex-col bg-base-200 hover:bg-base-300 transition-colors cursor-pointer text-left rounded-lg p-3"
          onclick={() => onSelect?.(album)}
        >
          <LazyImage
            coverArtId={album.cover_art_id}
            size={200}
            alt={album.name}
            class="w-full mb-2"
          />
          <h3 class="font-medium text-sm truncate w-full">{album.name}</h3>
          <p class="text-xs opacity-70 truncate w-full h-4">
            {album.artistName ?? ""}
          </p>
          <p class="text-xs opacity-50">
            {#if album.year}
              {album.year} &middot;
            {/if}
            {album.song_count}
            {album.song_count === 1 ? "song" : "songs"}
          </p>
        </button>
      {/each}
    </div>
  {:else}
    <div class="text-center text-sm opacity-50 py-8">No albums found.</div>
  {/if}
</div>
