<script lang="ts">
  import type { Artist } from "$lib/types";
  import LazyImage from "$lib/components/LazyImage.svelte";
  import { getSongs } from "$lib/api/commands";
  import { queue } from "$lib/stores/queue.svelte";
  import { ListEnd, ListPlus } from "lucide-svelte";

  interface Props {
    artists: Artist[];
    onSelect?: (artist: Artist) => void;
  }

  let { artists, onSelect }: Props = $props();

  let contextMenu = $state<{
    x: number;
    y: number;
    artist: Artist;
  } | null>(null);

  $effect(() => {
    if (contextMenu) {
      const handler = () => {
        contextMenu = null;
      };
      window.addEventListener("click", handler);
      return () => window.removeEventListener("click", handler);
    }
  });

  function handleContextMenu(e: MouseEvent, artist: Artist) {
    e.preventDefault();
    contextMenu = { x: e.clientX, y: e.clientY, artist };
  }

  async function handlePlayNext() {
    if (!contextMenu) return;
    const songs = await getSongs(undefined, contextMenu.artist.id);
    await queue.playNextSongs(songs);
    contextMenu = null;
  }

  async function handleAddToQueue() {
    if (!contextMenu) return;
    const songs = await getSongs(undefined, contextMenu.artist.id);
    await queue.addSongs(songs);
    contextMenu = null;
  }
</script>

<div class="flex-1 overflow-auto p-4">
  {#if artists.length > 0}
    <div
      class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-2"
    >
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

{#if contextMenu}
  <div
    class="ctx-menu"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
  >
    <button
      class="ctx-item"
      onclick={(e) => {
        e.stopPropagation();
        handlePlayNext();
      }}
    >
      <ListEnd class="size-3" />
      Play Next
    </button>
    <button
      class="ctx-item"
      onclick={(e) => {
        e.stopPropagation();
        handleAddToQueue();
      }}
    >
      <ListPlus class="size-3" />
      Add to Queue
    </button>
  </div>
{/if}

<style>
  .ctx-menu {
    position: fixed;
    z-index: 1000;
    min-width: 160px;
    max-width: 220px;
    padding: 4px;
    background: oklch(98% 0.002 250);
    border: 1px solid oklch(82% 0.008 250);
    border-radius: 6px;
    box-shadow:
      0 6px 20px oklch(0% 0 0 / 0.15),
      0 1px 3px oklch(0% 0 0 / 0.1);
  }

  .ctx-item {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 4px 8px;
    font-size: 0.75rem;
    line-height: 1.25;
    text-align: left;
    color: oklch(22% 0.01 250);
    border: none;
    background: transparent;
    border-radius: 4px;
    cursor: default;
    white-space: nowrap;
  }

  .ctx-item:hover {
    background: linear-gradient(
      to bottom,
      oklch(58% 0.2 250),
      oklch(52% 0.22 250)
    );
    color: white;
  }
</style>
