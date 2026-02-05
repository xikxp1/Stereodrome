<script lang="ts">
  import type { Artist } from "$lib/types";
  import LazyImage from "$lib/components/LazyImage.svelte";
  import { getSongs } from "$lib/api/commands";
  import { queue } from "$lib/stores/queue.svelte";
  import { showQueueableContextMenu } from "$lib/services/contextMenu";

  interface Props {
    artists: Artist[];
    onSelect?: (artist: Artist) => void;
  }

  let { artists, onSelect }: Props = $props();

  async function handleContextMenu(e: MouseEvent, artist: Artist) {
    e.preventDefault();
    await showQueueableContextMenu({
      onPlayNext: async () => {
        const songs = await getSongs(undefined, artist.id);
        await queue.playNextSongs(songs);
      },
      onAddToQueue: async () => {
        const songs = await getSongs(undefined, artist.id);
        await queue.addSongs(songs);
      },
    });
  }
</script>

<div class="flex-1 overflow-auto p-4">
  {#if artists.length > 0}
    <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
      {#each artists as artist (artist.id)}
        <button
          class="flex flex-col bg-base-200 hover:bg-base-300 transition-colors cursor-pointer text-left rounded-lg p-3"
          onclick={() => onSelect?.(artist)}
          oncontextmenu={(e) => handleContextMenu(e, artist)}
        >
          <LazyImage
            coverArtId={artist.cover_art_id}
            size={200}
            alt={artist.name}
            class="w-full mb-2"
          />
          <h3 class="font-medium text-sm truncate w-full">{artist.name}</h3>
          <p class="text-xs opacity-50">
            {artist.album_count}
            {artist.album_count === 1 ? "album" : "albums"}
          </p>
        </button>
      {/each}
    </div>
  {:else}
    <div class="text-center text-sm opacity-50 py-8">No artists found.</div>
  {/if}
</div>
