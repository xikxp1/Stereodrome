<script lang="ts">
  import { tick } from "svelte";
  import { queue } from "$lib/stores/queue.svelte";
  import { createVirtualizer } from "@tanstack/svelte-virtual";
  import type { QueueItem } from "$lib/types";
  import {
    Dices,
    X,
    Shuffle,
    Repeat,
    Repeat1,
    ListMusic,
    Volume2,
    LocateFixed,
    Trash2,
    GripVertical,
  } from "lucide-svelte";

  interface Props {
    onItemClick?: (songId: string) => void;
  }

  let { onItemClick }: Props = $props();

  let scrollContainer: HTMLDivElement | null = $state(null);
  let draggedIndex: number | null = $state(null);
  let dragOverIndex: number | null = $state(null);
  let dragOverPlacement: "before" | "after" = $state("before");
  let pointerDragging = $state(false);
  let suppressNextAutoScroll = $state(false);

  const ROW_HEIGHT = 40;

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
  let prevCurrentIndex: number | null = null;
  let prevItemsRef: QueueItem[] | null = null;
  let prevShuffle = false;
  let prevRepeatMode = queue.repeatMode;
  let prevScrollContainer: HTMLDivElement | null = null;

  // Scroll to current item when queue changes or virtualizer is recreated
  $effect(() => {
    const index = queue.currentIndex;
    const items = queue.items;
    const shuffle = queue.shuffle;
    const repeatMode = queue.repeatMode;
    const container = scrollContainer;

    // Detect changes that should trigger scroll-to-current
    const indexChanged = index !== prevCurrentIndex;
    const itemsRefChanged = items !== prevItemsRef && items.length > 0;
    const shuffleChanged = shuffle !== prevShuffle;
    const repeatModeChanged = repeatMode !== prevRepeatMode;
    const containerJustMounted =
      container !== null && prevScrollContainer === null;
    const queueChanged =
      indexChanged || itemsRefChanged || shuffleChanged || repeatModeChanged;

    if (suppressNextAutoScroll && queueChanged) {
      suppressNextAutoScroll = false;
    } else if (
      (queueChanged || containerJustMounted) &&
      index !== null &&
      container
    ) {
      // Use requestAnimationFrame to ensure virtualizer is ready
      requestAnimationFrame(() => {
        $virtualizer.scrollToIndex(index, { align: "center" });
      });
    }

    prevCurrentIndex = index;
    prevItemsRef = items;
    prevShuffle = shuffle;
    prevRepeatMode = repeatMode;
    prevScrollContainer = container;
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

  async function handleRerollNext() {
    await queue.rerollNext();
  }

  function handleScrollToCurrent() {
    if (queue.currentIndex !== null && scrollContainer) {
      $virtualizer.scrollToIndex(queue.currentIndex, {
        align: "center",
        behavior: "smooth",
      });
    }
  }

  function stopRowEvent(event: Event) {
    event.stopPropagation();
  }

  function clearDragState() {
    pointerDragging = false;
    draggedIndex = null;
    dragOverIndex = null;
    dragOverPlacement = "before";
  }

  function getDropDestination(
    from: number,
    targetIndex: number,
    placement: "before" | "after"
  ): number | null {
    const insertionIndex =
      placement === "before" ? targetIndex : targetIndex + 1;
    const adjustedIndex =
      from < insertionIndex ? insertionIndex - 1 : insertionIndex;
    const clampedIndex = Math.max(
      0,
      Math.min(adjustedIndex, queue.items.length - 1)
    );

    return clampedIndex === from ? null : clampedIndex;
  }

  function updateDropTargetFromClientY(clientY: number) {
    if (
      draggedIndex === null ||
      scrollContainer === null ||
      queue.items.length === 0
    ) {
      dragOverIndex = null;
      return;
    }

    const bounds = scrollContainer.getBoundingClientRect();
    const relativeY = clientY - bounds.top + scrollContainer.scrollTop;
    const maxY = queue.items.length * ROW_HEIGHT;
    const boundedY = Math.max(0, Math.min(relativeY, maxY));

    if (boundedY >= maxY) {
      dragOverIndex = queue.items.length - 1;
      dragOverPlacement = "after";
      return;
    }

    const index = Math.min(
      Math.floor(boundedY / ROW_HEIGHT),
      queue.items.length - 1
    );
    const rowOffset = boundedY - index * ROW_HEIGHT;

    dragOverIndex = index;
    dragOverPlacement = rowOffset < ROW_HEIGHT / 2 ? "before" : "after";
  }

  function isDropTarget(index: number, placement: "before" | "after"): boolean {
    if (
      draggedIndex === null ||
      dragOverIndex !== index ||
      dragOverPlacement !== placement
    ) {
      return false;
    }

    return getDropDestination(draggedIndex, index, placement) !== null;
  }

  function beginPointerDrag(index: number, event: MouseEvent) {
    event.preventDefault();
    stopRowEvent(event);
    pointerDragging = true;
    draggedIndex = index;
    dragOverIndex = index;
    dragOverPlacement = "before";
    updateDropTargetFromClientY(event.clientY);
  }

  function handlePointerMove(event: MouseEvent) {
    if (!pointerDragging || draggedIndex === null) {
      return;
    }

    updateDropTargetFromClientY(event.clientY);
  }

  async function handlePointerUp() {
    if (!pointerDragging || draggedIndex === null || dragOverIndex === null) {
      clearDragState();
      return;
    }

    const fromIndex = draggedIndex;
    const targetIndex = dragOverIndex;
    const placement = dragOverPlacement;
    const destination = getDropDestination(fromIndex, targetIndex, placement);

    clearDragState();

    if (destination === null) {
      return;
    }

    const preservedScrollTop = scrollContainer?.scrollTop ?? null;
    suppressNextAutoScroll = true;
    await queue.moveItem(fromIndex, destination);

    if (scrollContainer !== null && preservedScrollTop !== null) {
      await tick();
      scrollContainer.scrollTop = preservedScrollTop;
      requestAnimationFrame(() => {
        if (scrollContainer !== null) {
          scrollContainer.scrollTop = preservedScrollTop;
        }
      });
    }
  }
</script>

<svelte:window onmousemove={handlePointerMove} onmouseup={handlePointerUp} />

<div
  class="queue-panel flex flex-col h-full bg-base-100 border-l border-base-300"
  class:drag-active={pointerDragging}
>
  <!-- Header -->
  <div
    class="flex items-center gap-1.5 px-2 py-1.5 border-b border-base-300 bg-linear-to-b from-base-200 to-base-300"
  >
    <div class="flex items-center gap-1 min-w-0 flex-1 overflow-hidden">
      <ListMusic class="w-3.5 h-3.5 text-base-content/60 shrink-0" />
      <span class="text-xs font-medium truncate">Queue</span>
      <span class="text-[10px] text-base-content/50 shrink-0"
        >{queue.items.length}</span
      >
    </div>
    <div class="flex items-center shrink-0">
      <!-- Shuffle -->
      <button
        class="queue-header-btn"
        class:active={queue.shuffle}
        onclick={handleToggleShuffle}
        title="Shuffle (S)"
      >
        <Shuffle class="w-3 h-3" />
      </button>
      <!-- Repeat -->
      <button
        class="queue-header-btn"
        class:active={queue.repeatMode !== "Off"}
        onclick={handleCycleRepeat}
        title="Repeat: {queue.repeatMode} (R)"
      >
        {#if queue.repeatMode === "One"}
          <Repeat1 class="w-3 h-3" />
        {:else}
          <Repeat class="w-3 h-3" />
        {/if}
      </button>
      <!-- Reroll -->
      <button
        class="queue-header-btn"
        onclick={handleRerollNext}
        title="Reroll next track (D)"
        aria-label="Reroll next track"
        disabled={!queue.canRerollNext}
      >
        <Dices class="w-3 h-3" />
      </button>
      <!-- Scroll to current -->
      <button
        class="queue-header-btn"
        onclick={handleScrollToCurrent}
        title="Scroll to current"
        disabled={queue.currentIndex === null}
      >
        <LocateFixed class="w-3 h-3" />
      </button>
      <!-- Clear -->
      <button
        class="queue-header-btn"
        onclick={handleClearQueue}
        title="Clear queue"
        disabled={queue.items.length === 0}
      >
        <Trash2 class="w-3 h-3" />
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
            class:dragging={draggedIndex === index}
            class:drop-above={isDropTarget(index, "before")}
            class:drop-below={isDropTarget(index, "after")}
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
            <button
              class="queue-item-drag-handle"
              onclick={stopRowEvent}
              onkeydown={stopRowEvent}
              onmousedown={(e) => beginPointerDrag(index, e)}
              title="Drag to reorder"
              aria-label="Drag to reorder {item.title}"
            >
              <GripVertical class="w-3 h-3" />
            </button>
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
              <X class="w-3 h-3" />
            </button>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .queue-panel {
    width: clamp(220px, 18vw, 320px);
    min-width: 220px;
  }

  .queue-panel.drag-active {
    user-select: none;
  }

  .queue-header-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 3px;
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
    position: relative;
    align-items: center;
    gap: 0.25rem;
    padding: 0.375rem 0.375rem;
    cursor: pointer;
    border-bottom: 1px solid oklch(94% 0.003 250);
  }

  .queue-item:hover {
    background: oklch(96% 0.005 250);
  }

  .queue-item.playing {
    background: oklch(94% 0.02 250);
  }

  .queue-item.dragging {
    opacity: 0.45;
  }

  .queue-item.drop-above::before,
  .queue-item.drop-below::after {
    content: "";
    position: absolute;
    left: 0.5rem;
    right: 0.5rem;
    height: 2px;
    border-radius: 999px;
    background: oklch(58% 0.19 255);
    pointer-events: none;
  }

  .queue-item.drop-above::before {
    top: 0;
  }

  .queue-item.drop-below::after {
    bottom: -1px;
  }

  .queue-item-index {
    width: 1.125rem;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .queue-item-drag-handle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 0.875rem;
    color: oklch(50% 0.01 250);
    cursor: grab;
    flex-shrink: 0;
    transition: color 0.15s;
  }

  .queue-item-drag-handle:hover {
    color: oklch(35% 0.02 250);
  }

  .queue-item-drag-handle:active {
    cursor: grabbing;
  }

  .queue-item-info {
    flex: 1;
    min-width: 0;
  }

  .queue-item-title {
    font-size: 0.75rem;
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
    min-width: 2rem;
    text-align: right;
  }

  .queue-item-remove {
    opacity: 0;
    padding: 0.0625rem;
    border-radius: 0.125rem;
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
