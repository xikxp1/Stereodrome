<script lang="ts">
  import { queue } from "$lib/stores/queue.svelte";
  import { createVirtualizer } from "@tanstack/svelte-virtual";
  import {
    X,
    Shuffle,
    Repeat,
    Repeat1,
    ListMusic,
    Volume2,
    LocateFixed,
    Trash2,
  } from "lucide-svelte";

  interface Props {
    onItemClick?: (songId: string) => void;
  }

  let { onItemClick }: Props = $props();

  let scrollContainer: HTMLDivElement | null = $state(null);

  const ROW_HEIGHT = 48;

  const virtualizer = $derived(
    createVirtualizer<HTMLDivElement, HTMLDivElement>({
      count: queue.items.length,
      getScrollElement: scrollContainer ? () => scrollContainer : () => null,
      estimateSize: () => ROW_HEIGHT,
      overscan: 5,
    })
  );

  const virtualItems = $derived($virtualizer.getVirtualItems());
  const totalSize = $derived($virtualizer.getTotalSize());

  // Track previous values to detect actual changes
  let prevCurrentIndex: number | null = $state(null);
  let prevItemsLength = $state(0);
  let prevShuffle = $state(false);
  let prevRepeatMode = $state(queue.repeatMode);

  // Scroll to current item only when queue changes (not on every render)
  $effect(() => {
    const index = queue.currentIndex;
    const itemsLength = queue.items.length;
    const shuffle = queue.shuffle;
    const repeatMode = queue.repeatMode;

    // Only scroll if currentIndex changed, items were added, or shuffle/repeat toggled
    const indexChanged = index !== prevCurrentIndex;
    const itemsChanged = itemsLength !== prevItemsLength && itemsLength > 0;
    const shuffleChanged = shuffle !== prevShuffle;
    const repeatModeChanged = repeatMode !== prevRepeatMode;

    if (
      (indexChanged || itemsChanged || shuffleChanged || repeatModeChanged) &&
      index !== null &&
      scrollContainer
    ) {
      // Use requestAnimationFrame to ensure virtualizer is ready
      requestAnimationFrame(() => {
        $virtualizer.scrollToIndex(index, { align: "center" });
      });
    }

    prevCurrentIndex = index;
    prevItemsLength = itemsLength;
    prevShuffle = shuffle;
    prevRepeatMode = repeatMode;
  });

  function formatDuration(seconds: number): string {
    if (!seconds) return "--:--";
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  }

  async function handlePlayItem(index: number) {
    await queue.playQueueItem(index);
  }

  async function handleRemoveItem(index: number, e: MouseEvent) {
    e.stopPropagation();
    await queue.removeFromQueue(index);
  }

  async function handleClearQueue() {
    await queue.clearQueue();
  }

  async function handleToggleShuffle() {
    await queue.toggleShuffle();
  }

  async function handleCycleRepeat() {
    await queue.cycleRepeatMode();
  }

  function handleScrollToCurrent() {
    if (queue.currentIndex !== null && scrollContainer) {
      $virtualizer.scrollToIndex(queue.currentIndex, { align: "center" });
    }
  }
</script>

<div
  class="queue-panel flex flex-col h-full bg-base-100 border-l border-base-300"
>
  <!-- Header -->
  <div
    class="flex items-center gap-2 px-2 py-2 border-b border-base-300 bg-gradient-to-b from-base-200 to-base-300"
  >
    <div class="flex items-center gap-1.5 min-w-0 flex-1 overflow-hidden">
      <ListMusic class="w-4 h-4 text-base-content/60 flex-shrink-0" />
      <span class="text-sm font-medium truncate">Queue</span>
      <span class="text-xs text-base-content/50 flex-shrink-0"
        >{queue.items.length}</span
      >
    </div>
    <div class="flex items-center flex-shrink-0">
      <!-- Shuffle -->
      <button
        class="queue-header-btn"
        class:active={queue.shuffle}
        onclick={handleToggleShuffle}
        title="Shuffle"
      >
        <Shuffle class="w-3.5 h-3.5" />
      </button>
      <!-- Repeat -->
      <button
        class="queue-header-btn"
        class:active={queue.repeatMode !== "Off"}
        onclick={handleCycleRepeat}
        title="Repeat: {queue.repeatMode}"
      >
        {#if queue.repeatMode === "One"}
          <Repeat1 class="w-3.5 h-3.5" />
        {:else}
          <Repeat class="w-3.5 h-3.5" />
        {/if}
      </button>
      <!-- Scroll to current -->
      <button
        class="queue-header-btn"
        onclick={handleScrollToCurrent}
        title="Scroll to current"
        disabled={queue.currentIndex === null}
      >
        <LocateFixed class="w-3.5 h-3.5" />
      </button>
      <!-- Clear -->
      <button
        class="queue-header-btn"
        onclick={handleClearQueue}
        title="Clear queue"
        disabled={queue.items.length === 0}
      >
        <Trash2 class="w-3.5 h-3.5" />
      </button>
    </div>
  </div>

  <!-- Queue list -->
  {#if queue.items.length === 0}
    <div
      class="flex-1 flex flex-col items-center justify-center text-base-content/40 gap-2"
    >
      <ListMusic class="w-8 h-8" />
      <span class="text-sm">Queue is empty</span>
    </div>
  {:else}
    <div bind:this={scrollContainer} class="flex-1 overflow-auto">
      <div style="height: {totalSize}px; width: 100%; position: relative;">
        {#each virtualItems as row (row.index)}
          {@const item = queue.items[row.index]}
          {@const index = row.index}
          {@const isPlaying = queue.currentIndex === index}
          <div
            class="queue-item"
            class:playing={isPlaying}
            role="row"
            tabindex="0"
            onclick={() => onItemClick?.(item.song_id)}
            ondblclick={() => handlePlayItem(index)}
            onkeydown={(e) => e.key === "Enter" && handlePlayItem(index)}
            style="position: absolute; top: 0; left: 0; width: 100%; height: {row.size}px; transform: translateY({row.start}px);"
          >
            <div class="queue-item-index">
              {#if isPlaying}
                <Volume2 class="w-3 h-3 animate-pulse text-primary" />
              {:else}
                <span class="text-xs text-base-content/40">{index + 1}</span>
              {/if}
            </div>
            <div class="queue-item-info">
              <div class="queue-item-title" class:text-primary={isPlaying}>
                {item.title}
              </div>
              <div class="queue-item-artist">
                {item.artist}
              </div>
            </div>
            <div class="queue-item-duration">
              {formatDuration(item.duration)}
            </div>
            <button
              class="queue-item-remove"
              onclick={(e) => handleRemoveItem(index, e)}
              title="Remove from queue"
            >
              <X class="w-3.5 h-3.5" />
            </button>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .queue-panel {
    width: 280px;
    min-width: 280px;
  }

  .queue-header-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 4px;
    color: oklch(50% 0.01 250);
    transition:
      background-color 0.15s,
      color 0.15s;
  }

  .queue-header-btn:hover {
    background: oklch(90% 0.01 250);
    color: oklch(30% 0.01 250);
  }

  .queue-header-btn:disabled {
    opacity: 0.4;
    pointer-events: none;
  }

  .queue-header-btn.active {
    background: oklch(85% 0.05 250);
    color: oklch(45% 0.15 250);
  }

  .queue-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    cursor: pointer;
    border-bottom: 1px solid oklch(94% 0.003 250);
  }

  .queue-item:hover {
    background: oklch(96% 0.005 250);
  }

  .queue-item.playing {
    background: oklch(94% 0.02 250);
  }

  .queue-item-index {
    width: 1.5rem;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .queue-item-info {
    flex: 1;
    min-width: 0;
  }

  .queue-item-title {
    font-size: 0.8125rem;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .queue-item-artist {
    font-size: 0.6875rem;
    color: oklch(50% 0.01 250);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .queue-item-duration {
    font-size: 0.6875rem;
    color: oklch(50% 0.01 250);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  .queue-item-remove {
    opacity: 0;
    padding: 0.25rem;
    border-radius: 0.25rem;
    color: oklch(50% 0.01 250);
    transition:
      opacity 0.15s,
      background-color 0.15s;
    flex-shrink: 0;
  }

  .queue-item:hover .queue-item-remove {
    opacity: 1;
  }

  .queue-item-remove:hover {
    background: oklch(90% 0.01 250);
    color: oklch(30% 0.01 250);
  }
</style>
