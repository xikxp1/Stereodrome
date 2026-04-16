<script lang="ts">
  import type { Album, AlbumListEntry } from "$lib/types";
  import LazyImage from "$lib/components/LazyImage.svelte";
  import { getSongs } from "$lib/api/commands";
  import { queue } from "$lib/stores/queue.svelte";
  import { showQueueableContextMenu } from "$lib/services/contextMenu";
  import {
    createVirtualizer,
    type SvelteVirtualizer,
    type VirtualItem,
  } from "@tanstack/svelte-virtual";

  type AlbumGridItem = Album | AlbumListEntry;

  interface Props {
    albums: AlbumGridItem[];
    onSelect?: (album: AlbumGridItem) => void;
    onNavigateToArtist?: (album: AlbumGridItem) => void;
  }

  let { albums, onSelect, onNavigateToArtist }: Props = $props();
  let scrollContainer: HTMLDivElement | null = $state(null);
  let containerWidth = $state(0);

  const HORIZONTAL_PADDING = 32;
  const GRID_GAP = 16;
  const ESTIMATED_ROW_HEIGHT = 320;

  const contentWidth = $derived(
    Math.max(0, containerWidth - HORIZONTAL_PADDING)
  );
  const columns = $derived(
    contentWidth >= 1024 ? 5 : contentWidth >= 768 ? 4 : 3
  );
  const rowCount = $derived(Math.ceil(albums.length / columns));
  const virtualizer = $derived(
    createVirtualizer<HTMLDivElement, HTMLDivElement>({
      count: rowCount,
      getScrollElement: scrollContainer ? () => scrollContainer : () => null,
      estimateSize: () => ESTIMATED_ROW_HEIGHT,
      getItemKey: (index) => `album-row-${index}`,
      overscan: 3,
    })
  );
  const virtualItems = $derived($virtualizer.getVirtualItems());
  const totalSize = $derived($virtualizer.getTotalSize());

  $effect(() => {
    void columns;

    if (!scrollContainer || rowCount === 0) {
      return;
    }

    requestAnimationFrame(() => {
      $virtualizer.measure();
    });
  });

  function getAlbumsForRow(rowIndex: number) {
    const startIndex = rowIndex * columns;
    return albums.slice(startIndex, startIndex + columns);
  }

  function getRowStyle(item: VirtualItem) {
    return `position: absolute; top: 0; left: 0; width: 100%; transform: translateY(${item.start}px); display: grid; grid-template-columns: repeat(${columns}, minmax(0, 1fr)); gap: ${GRID_GAP}px;`;
  }

  function measureRow(
    node: HTMLDivElement,
    instance: SvelteVirtualizer<HTMLDivElement, HTMLDivElement>
  ) {
    instance.measureElement(node);

    return {
      update(nextInstance: SvelteVirtualizer<HTMLDivElement, HTMLDivElement>) {
        nextInstance.measureElement(node);
      },
    };
  }

  async function handleContextMenu(e: MouseEvent, album: AlbumGridItem) {
    e.preventDefault();
    const artistId = album.artist_id;
    await showQueueableContextMenu({
      onPlayNext: async () => {
        const songs = await getSongs(album.id);
        await queue.playNextSongs(songs);
      },
      onAddToQueue: async () => {
        const songs = await getSongs(album.id);
        await queue.addSongs(songs);
      },
      onGoToArtist: artistId
        ? () => {
            onNavigateToArtist?.(album);
          }
        : undefined,
    });
  }
</script>

<div
  bind:this={scrollContainer}
  bind:clientWidth={containerWidth}
  class="flex-1 overflow-auto p-4"
>
  {#if albums.length > 0}
    <div class="relative" style="height: {totalSize}px;">
      {#each virtualItems as item (item.key)}
        <div
          data-index={item.index}
          use:measureRow={$virtualizer}
          style={getRowStyle(item)}
        >
          {#each getAlbumsForRow(item.index) as album (album.id)}
            <button
              class="virtual-grid-card flex flex-col bg-base-200 hover:bg-base-300 transition-colors cursor-pointer text-left rounded-lg p-3"
              onclick={() => onSelect?.(album)}
              oncontextmenu={(e) => handleContextMenu(e, album)}
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
                {#if album.song_count != null}
                  {album.song_count}
                  {album.song_count === 1 ? "song" : "songs"}
                {:else if album.duration}
                  {Math.floor(album.duration / 60)} min
                {/if}
              </p>
            </button>
          {/each}
        </div>
      {/each}
    </div>
  {:else}
    <div class="text-center text-sm opacity-50 py-8">No albums found.</div>
  {/if}
</div>

<style>
  .virtual-grid-card {
    min-width: 0;
  }
</style>
