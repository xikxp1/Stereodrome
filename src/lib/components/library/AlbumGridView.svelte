<script lang="ts">
  import type { Album, AlbumListEntry } from "$lib/types";
  import LazyImage from "$lib/components/LazyImage.svelte";
  import { getSongs } from "$lib/api/commands";
  import { queue } from "$lib/stores/queue.svelte";
  import { showQueueableContextMenu } from "$lib/services/contextMenu";
  import { untrack } from "svelte";
  import { on } from "svelte/events";
  import {
    createVirtualizer,
    type SvelteVirtualizer,
  } from "@tanstack/svelte-virtual";

  type AlbumGridItem = Album | AlbumListEntry;

  interface Props {
    albums: AlbumGridItem[];
    totalCount?: number;
    hasMore?: boolean;
    isLoadingMore?: boolean;
    onLoadMore?: () => void | Promise<void>;
    onSelect?: (album: AlbumGridItem) => void;
    onNavigateToArtist?: (album: AlbumGridItem) => void;
    restoreScrollOffset?: number | null;
    onScrollOffsetChange?: (offset: number) => void;
  }

  let {
    albums,
    totalCount = 0,
    hasMore = false,
    isLoadingMore = false,
    onLoadMore,
    onSelect,
    onNavigateToArtist,
    restoreScrollOffset = null,
    onScrollOffsetChange,
  }: Props = $props();
  let scrollContainer: HTMLDivElement | null = $state(null);
  let containerWidth = $state(0);

  const HORIZONTAL_PADDING = 32;
  const ESTIMATED_ROW_HEIGHT = 320;

  const contentWidth = $derived(
    Math.max(0, containerWidth - HORIZONTAL_PADDING)
  );
  const columns = $derived(
    contentWidth >= 1024 ? 5 : contentWidth >= 768 ? 4 : 3
  );
  // Total rows the virtualizer should account for (includes unloaded rows)
  const totalAlbumCount = $derived(totalCount > 0 ? totalCount : albums.length);
  const totalRowCount = $derived(Math.ceil(totalAlbumCount / columns));
  // Rows that have been loaded with actual data
  const loadedRowCount = $derived(Math.ceil(albums.length / columns));

  const virtualizer = createVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: 0,
    getScrollElement: () => null,
    estimateSize: () => ESTIMATED_ROW_HEIGHT,
    getItemKey: (index) => `album-row-${index}`,
    overscan: 3,
  });

  // Reactively update virtualizer options without recreating the instance.
  // Use untrack to avoid infinite loop: setOptions updates the store,
  // which would re-trigger this effect if $virtualizer were tracked.
  $effect(() => {
    const count = totalRowCount;
    const scrollEl = scrollContainer;
    untrack(() => {
      $virtualizer.setOptions({
        count,
        getScrollElement: () => scrollEl ?? null,
      });
    });
  });

  const virtualItems = $derived($virtualizer.getVirtualItems());
  const totalSize = $derived($virtualizer.getTotalSize());

  $effect(() => {
    void columns;

    if (!scrollContainer || totalRowCount === 0) {
      return;
    }

    requestAnimationFrame(() => {
      $virtualizer.measure();
    });
  });

  // Infinite scroll: load more albums when near the end of loaded data
  $effect(() => {
    if (!onLoadMore || isLoadingMore || !hasMore) return;

    const range = $virtualizer.range;
    if (!range || albums.length === 0) return;

    if (range.endIndex >= loadedRowCount - 2) {
      void onLoadMore();
    }
  });

  // Track scroll position and report to parent
  $effect(() => {
    if (!scrollContainer) return undefined;
    const el = scrollContainer;
    const handleScroll = () => {
      onScrollOffsetChange?.(el.scrollTop);
    };
    return on(el, "scroll", handleScroll);
  });

  // Restore scroll position on mount
  let scrollRestored = $state(false);

  $effect(() => {
    if (
      restoreScrollOffset &&
      !scrollRestored &&
      scrollContainer &&
      totalRowCount > 0
    ) {
      const offset = restoreScrollOffset;
      requestAnimationFrame(() => {
        $virtualizer.scrollToOffset(offset, { align: "start" });
        scrollRestored = true;
      });
    }
  });

  function getAlbumsForRow(rowIndex: number) {
    const startIndex = rowIndex * columns;
    return albums.slice(startIndex, startIndex + columns);
  }

  function isRowLoaded(rowIndex: number): boolean {
    return rowIndex < loadedRowCount;
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
      ...(artistId
        ? {
            onGoToArtist: () => {
              onNavigateToArtist?.(album);
            },
          }
        : {}),
    });
  }
</script>

<div
  bind:this={scrollContainer}
  bind:clientWidth={containerWidth}
  class="flex-1 overflow-auto p-4"
>
  {#if albums.length > 0}
    <div class="relative" style:height={`${totalSize}px`}>
      {#each virtualItems as item (item.key)}
        <div
          class="virtual-grid-row"
          data-index={item.index}
          use:measureRow={$virtualizer}
          style:transform={`translateY(${item.start}px)`}
          style:grid-template-columns={`repeat(${columns}, minmax(0, 1fr))`}
        >
          {#if isRowLoaded(item.index)}
            {#each getAlbumsForRow(item.index) as album (album.id)}
              <button
                type="button"
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
                <h3 class="font-medium text-sm truncate w-full">
                  {album.name}
                </h3>
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
          {:else}
            <!-- Skeleton rows for unloaded albums -->
            {#each Array(columns)}
              <div
                class="virtual-grid-card flex flex-col bg-base-200 rounded-lg p-3 animate-pulse"
              >
                <div class="aspect-square bg-base-300 rounded mb-2"></div>
                <div class="h-4 bg-base-300 rounded w-3/4 mb-1"></div>
                <div class="h-3 bg-base-300 rounded w-1/2"></div>
              </div>
            {/each}
          {/if}
        </div>
      {/each}

      {#if isLoadingMore}
        <div
          class="absolute left-0 right-0 flex justify-center py-4"
          style="transform: translateY({totalSize}px);"
        >
          <span class="loading loading-spinner loading-md opacity-50"></span>
        </div>
      {/if}
    </div>
  {:else}
    <div class="text-center text-sm opacity-50 py-8">No albums found.</div>
  {/if}
</div>

<style>
  .virtual-grid-row {
    position: absolute;
    top: 0;
    left: 0;
    display: grid;
    width: 100%;
    gap: 16px;
  }

  .virtual-grid-card {
    min-width: 0;
  }
</style>
