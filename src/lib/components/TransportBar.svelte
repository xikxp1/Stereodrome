<script lang="ts">
  import { searchStore } from "$lib/stores/search.svelte";
  import { queue } from "$lib/stores/queue.svelte";
  import { updater } from "$lib/stores/updater.svelte";
  import NowPlayingCenter from "./NowPlayingCenter.svelte";
  import {
    Dices,
    SkipBack,
    Play,
    Pause,
    SkipForward,
    Volume1,
    Volume2,
    VolumeX,
    ListMusic,
    Shuffle,
    Repeat,
    Repeat1,
    Settings,
    LoaderCircle,
  } from "lucide-svelte";

  let volumeDropdownOpen = $state(false);
  let volumeAdjusting = $state(false);
  let volumeAdjustTimeout: ReturnType<typeof setTimeout> | null = null;

  const isMac =
    typeof navigator !== "undefined" &&
    /Mac|iPhone|iPad|iPod/.test(navigator.userAgent);
  const modKey = isMac ? "⌘" : "Ctrl+";

  // Derived tooltip text for prev/next/play buttons
  const prevTooltip = $derived.by(() => {
    const prev = queue.previousSong;
    if (prev) {
      return `Previous (${modKey}←)\n${prev.artist} - ${prev.title}`;
    }
    return `Previous (${modKey}←)`;
  });

  const nextTooltip = $derived.by(() => {
    const next = queue.nextSong;
    if (next) {
      return `Next (${modKey}→)\n${next.artist} - ${next.title}`;
    }
    return `Next (${modKey}→)`;
  });

  const playTooltip = $derived.by(() => {
    if (isPlaying) {
      return "Pause (Space)";
    }
    const current = queue.currentSong;
    if (current) {
      return `Play (Space)\n${current.artist} - ${current.title}`;
    }
    return "Play (Space)";
  });

  function handleVolumeChange(newVolume: number) {
    onVolumeChange?.(newVolume);
    volumeAdjusting = true;
    if (volumeAdjustTimeout) clearTimeout(volumeAdjustTimeout);
    volumeAdjustTimeout = setTimeout(() => {
      volumeAdjusting = false;
    }, 1000);
  }

  interface Props {
    isPlaying?: boolean;
    currentTrack?: { title: string; artist: string; album: string } | null;
    currentTime?: number;
    duration?: number;
    volume?: number;
    volumeAdjusting?: boolean;
    queueOpen?: boolean;
    coverArtUrl?: string | null;
    filteredSongsCount?: number;
    searchInputRef?: HTMLInputElement | null;
    onPlayPause?: () => void;
    onPrevious?: () => void;
    onNext?: () => void;
    onSeek?: (time: number) => void;
    onVolumeChange?: (volume: number) => void;
    onQueueToggle?: () => void;
    onCoverArtClick?: () => void;
    onSettingsClick?: () => void;
    onMiniPlayerToggle?: () => void;
  }

  let {
    isPlaying = false,
    currentTrack = null,
    currentTime = 0,
    duration = 0,
    volume: volumeProp = 80,
    volumeAdjusting: volumeAdjustingProp = false,
    queueOpen = false,
    coverArtUrl = null,
    filteredSongsCount = 0,
    searchInputRef = $bindable(null),
    onPlayPause,
    onPrevious,
    onNext,
    onSeek,
    onVolumeChange,
    onQueueToggle,
    onCoverArtClick,
    onSettingsClick,
    onMiniPlayerToggle,
  }: Props = $props();

  // Ensure volume is always a whole number for display
  const volume = $derived(Math.round(volumeProp));
  const volumeRatio = $derived(volume / 100);
  const isMutedWhilePlaying = $derived(isPlaying && volume === 0);

  function handleInput(e: Event) {
    const input = e.target as HTMLInputElement;
    searchStore.setQuery(input.value);
  }
</script>

<!-- Toolbar -->
<div
  class="relative flex h-20 items-center justify-between border-b border-base-300 bg-linear-to-b from-base-200 to-base-300 px-3 select-none"
>
  <!-- Left: Playback Controls -->
  <div class="z-10 flex items-center gap-4">
    <!-- Playback Controls -->
    <div class="relative flex items-center">
      <div class="flex rounded bg-base-300 p-0.5 shadow-sm">
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded-l bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content active:bg-base-300 disabled:cursor-not-allowed disabled:text-base-content/30 disabled:hover:bg-base-100"
          onclick={() => onPrevious?.()}
          disabled={!queue.hasPrevious}
          aria-label="Previous track"
          title={prevTooltip}
        >
          <SkipBack class="h-3 w-3" fill="currentColor" />
        </button>
        <button
          type="button"
          class="flex h-7 w-8 items-center justify-center border-x border-base-300 bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content active:bg-base-300 disabled:cursor-not-allowed disabled:text-base-content/30 disabled:hover:bg-base-100"
          onclick={() => onPlayPause?.()}
          disabled={queue.items.length === 0 &&
            !queue.currentSong &&
            filteredSongsCount === 0}
          aria-label={isPlaying ? "Pause" : "Play"}
          title={playTooltip}
        >
          {#if isPlaying}
            <Pause class="h-3.5 w-3.5" fill="currentColor" />
          {:else}
            <Play class="ml-0.5 h-3.5 w-3.5" fill="currentColor" />
          {/if}
        </button>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded-r bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content active:bg-base-300 disabled:cursor-not-allowed disabled:text-base-content/30 disabled:hover:bg-base-100"
          onclick={() => onNext?.()}
          disabled={!queue.hasNext}
          aria-label="Next track"
          title={nextTooltip}
        >
          <SkipForward class="h-3 w-3" fill="currentColor" />
        </button>
      </div>
      <button
        type="button"
        class="absolute right-0.5 top-full mt-0.75 flex h-4 w-7 items-center justify-center rounded border border-base-300 bg-base-100 text-base-content/55 transition-colors hover:bg-base-200 hover:text-base-content disabled:cursor-not-allowed disabled:text-base-content/25 disabled:hover:bg-base-100"
        onclick={() => queue.rerollNext()}
        disabled={!queue.canRerollNext}
        title="Reroll next track (D)"
        aria-label="Reroll next track (D)"
      >
        <Dices class="h-3 w-3" />
      </button>
    </div>

    <!-- Volume - Compact dropdown on small screens -->
    <div class="relative flex items-center gap-1.5 lg:hidden">
      <button
        type="button"
        class={[
          "flex h-7 w-7 items-center justify-center rounded bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content",
          isMutedWhilePlaying && "volume-alert-button",
        ]}
        onclick={() => (volumeDropdownOpen = !volumeDropdownOpen)}
        aria-label="Volume"
        title="Volume: {volume}% (M to mute, {modKey}↑/↓)"
      >
        {#if volume === 0}
          <span class={isMutedWhilePlaying ? "volume-alert-icon" : ""}>
            <VolumeX class="h-4 w-4" />
          </span>
        {:else if volume < 50}
          <Volume1 class="h-4 w-4" />
        {:else}
          <Volume2 class="h-4 w-4" />
        {/if}
      </button>
      {#if volumeDropdownOpen}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="fixed inset-0 z-40"
          onclick={() => (volumeDropdownOpen = false)}
          onkeydown={(e) => e.key === "Escape" && (volumeDropdownOpen = false)}
        ></div>
        <div
          class="absolute left-0 top-full z-50 mt-1 flex h-36 w-8 flex-col items-center rounded border border-base-300 bg-base-100 py-1 shadow-lg"
        >
          <span
            class="text-[10px] font-medium tabular-nums text-base-content/60"
            >{volume}%</span
          >
          <Volume2 class="h-3 w-3 shrink-0 text-base-content/40" />
          <div class="relative h-28 w-3">
            <div
              class="absolute top-2 left-1/2 h-[calc(100%-16px)] w-0.5 -translate-x-1/2 rounded-full bg-base-300"
            ></div>
            <div
              class={[
                "absolute bottom-2 left-1/2 w-0.5 -translate-x-1/2 rounded-full bg-base-content/30",
                isMutedWhilePlaying && "volume-alert-bar",
              ]}
              style="height: calc((100% - 16px) * {volumeRatio})"
            ></div>
            <input
              type="range"
              min="0"
              max="100"
              value={volume}
              class="absolute left-1/2 h-full w-20 -translate-x-1/2 origin-center -rotate-90 cursor-pointer opacity-0"
              oninput={(e) => handleVolumeChange(Number(e.currentTarget.value))}
              aria-label="Volume"
            />
            <div
              class={[
                "pointer-events-none absolute h-2.5 w-2.5 rounded-full border border-base-300 bg-base-100 shadow-sm",
                isMutedWhilePlaying && "volume-alert-thumb-mobile",
              ]}
              style="left: calc(50% - 5px); top: clamp(8px, calc(8px + (100% - 16px) * {1 -
                volumeRatio}), calc(100% - 18px))"
            ></div>
          </div>
          <Volume1 class="h-3 w-3 shrink-0 text-base-content/40" />
        </div>
      {/if}
    </div>

    <!-- Volume - Full slider on large screens -->
    <div class="relative hidden items-center gap-1.5 lg:flex">
      {#if volume === 0}
        <span
          class={[
            "shrink-0 text-base-content/40",
            isMutedWhilePlaying && "volume-alert-icon",
          ]}
        >
          <VolumeX class="h-3 w-3" />
        </span>
      {:else}
        <Volume1 class="h-3 w-3 shrink-0 text-base-content/40" />
      {/if}
      <div class="relative flex h-3 w-16 items-center">
        <div class="absolute h-0.5 w-full rounded-full bg-base-300"></div>
        <div
          class={[
            "absolute h-0.5 rounded-full bg-base-content/30",
            isMutedWhilePlaying && "volume-alert-bar",
          ]}
          style="width: {volume}%"
        ></div>
        <input
          type="range"
          min="0"
          max="100"
          value={volume}
          class="absolute w-full cursor-pointer opacity-0"
          oninput={(e) => handleVolumeChange(Number(e.currentTarget.value))}
          aria-label="Volume"
        />
        <div
          class={[
            "pointer-events-none absolute h-2.5 w-2.5 rounded-full border border-base-300 bg-base-100 shadow-sm",
            isMutedWhilePlaying && "volume-alert-thumb-desktop",
          ]}
          style="left: calc({volume}% - 5px)"
        ></div>
      </div>
      <Volume2 class="h-3 w-3 shrink-0 text-base-content/40" />
      {#if volumeAdjusting || volumeAdjustingProp}
        <span
          class="absolute -top-6 left-1/2 -translate-x-1/2 rounded bg-base-content px-1.5 py-0.5 text-[10px] font-medium tabular-nums text-base-100 shadow"
          >{volume}%</span
        >
      {/if}
    </div>

    <!-- Shuffle/Repeat -->
    <div class="flex items-center">
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded transition-colors disabled:cursor-not-allowed disabled:text-base-content/20 {queue.shuffle
          ? 'text-primary'
          : 'text-base-content/40 hover:text-base-content/70 disabled:hover:text-base-content/20'}"
        onclick={() => queue.toggleShuffle()}
        disabled={queue.items.length === 0}
        title="Shuffle (S)"
        aria-label="Toggle shuffle"
      >
        <Shuffle class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded transition-colors disabled:cursor-not-allowed disabled:text-base-content/20 {queue.repeatMode !==
        'Off'
          ? 'text-primary'
          : 'text-base-content/40 hover:text-base-content/70 disabled:hover:text-base-content/20'}"
        onclick={() => queue.cycleRepeatMode()}
        disabled={queue.items.length === 0}
        title="Repeat: {queue.repeatMode} (R)"
        aria-label="Cycle repeat mode"
      >
        {#if queue.repeatMode === "One"}
          <Repeat1 class="h-3.5 w-3.5" />
        {:else}
          <Repeat class="h-3.5 w-3.5" />
        {/if}
      </button>
    </div>
  </div>

  <NowPlayingCenter
    mode="toolbar"
    {currentTrack}
    {currentTime}
    {duration}
    {coverArtUrl}
    {...onSeek ? { onSeek } : {}}
    {...onCoverArtClick ? { onCoverArtClick } : {}}
    {...onMiniPlayerToggle ? { onMiniPlayerToggle } : {}}
  />

  <!-- Right: Settings + Queue Toggle + Search -->
  <div class="z-10 flex items-center gap-2">
    {#if updater.updateAvailable}
      <div class="indicator">
        <span class="indicator-item status status-primary"></span>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content"
          onclick={() => onSettingsClick?.()}
          aria-label="Settings"
          title={`Update available (v${updater.version}) - Settings (${modKey},)`}
        >
          <Settings class="h-4 w-4" />
        </button>
      </div>
    {:else}
      <button
        type="button"
        class="flex h-7 w-7 items-center justify-center rounded bg-base-100 text-base-content/70 transition-colors hover:bg-base-200 hover:text-base-content"
        onclick={() => onSettingsClick?.()}
        aria-label="Settings"
        title={`Settings (${modKey},)`}
      >
        <Settings class="h-4 w-4" />
      </button>
    {/if}
    <button
      type="button"
      class="flex h-7 w-7 items-center justify-center rounded transition-colors {queueOpen
        ? 'bg-primary text-primary-content'
        : 'bg-base-100 text-base-content/70 hover:bg-base-200 hover:text-base-content'}"
      onclick={() => onQueueToggle?.()}
      aria-label="Toggle queue"
      title="Queue (Q)"
    >
      <ListMusic class="h-4 w-4" />
    </button>
    <div class="relative">
      <input
        bind:this={searchInputRef}
        type="search"
        placeholder="Search ({modKey}K)"
        title="Search ({modKey}K)"
        value={searchStore.query}
        oninput={handleInput}
        autocomplete="off"
        autocorrect="off"
        autocapitalize="off"
        spellcheck="false"
        class="h-7 w-24 rounded-full border border-base-300 bg-base-100 px-3 text-xs outline-none transition-all duration-150 placeholder:text-base-content/40 focus:border-primary focus:ring-2 focus:ring-primary/20 sm:w-32 md:w-40 lg:w-44 lg:focus:w-52"
      />
      {#if searchStore.isSearching}
        <span
          class="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2"
        >
          <LoaderCircle class="h-4 w-4 animate-spin text-primary" />
        </span>
      {/if}
    </div>
  </div>
</div>

<style>
  .volume-alert-button {
    animation: volume-alert-button 1.05s cubic-bezier(0.4, 0, 0.2, 1) infinite;
    background-color: color-mix(
      in srgb,
      var(--color-error) 10%,
      var(--color-base-100)
    );
    color: color-mix(
      in srgb,
      var(--color-error) 65%,
      var(--color-base-content)
    );
  }

  .volume-alert-icon {
    animation: volume-alert-icon 0.75s ease-in-out infinite alternate;
    transform-origin: center;
    color: color-mix(
      in srgb,
      var(--color-error) 75%,
      var(--color-base-content)
    );
  }

  .volume-alert-bar {
    animation: volume-alert-bar 0.9s ease-in-out infinite;
  }

  .volume-alert-thumb-mobile {
    animation: volume-alert-thumb 0.9s ease-in-out infinite;
  }

  .volume-alert-thumb-desktop {
    animation: volume-alert-thumb-desktop 0.9s ease-in-out infinite;
  }

  @keyframes volume-alert-button {
    0%,
    100% {
      box-shadow:
        0 0 0 0 color-mix(in srgb, var(--color-error) 0%, transparent),
        0 0 0 1px color-mix(in srgb, var(--color-error) 20%, transparent);
      transform: translateY(0);
    }
    50% {
      box-shadow:
        0 0 0 6px color-mix(in srgb, var(--color-error) 20%, transparent),
        0 0 18px color-mix(in srgb, var(--color-error) 22%, transparent);
      transform: translateY(-0.5px);
    }
  }

  @keyframes volume-alert-icon {
    0% {
      opacity: 0.8;
      transform: scale(1) rotate(-8deg);
    }
    100% {
      opacity: 1;
      transform: scale(1.22) rotate(8deg);
    }
  }

  @keyframes volume-alert-bar {
    0%,
    100% {
      background-color: color-mix(
        in srgb,
        var(--color-error) 45%,
        var(--color-base-content)
      );
      box-shadow: 0 0 0 0 color-mix(in srgb, var(--color-error) 0%, transparent);
      opacity: 0.55;
    }
    50% {
      background-color: color-mix(
        in srgb,
        var(--color-error) 90%,
        var(--color-base-content)
      );
      box-shadow: 0 0 8px
        color-mix(in srgb, var(--color-error) 28%, transparent);
      opacity: 1;
    }
  }

  @keyframes volume-alert-thumb {
    0%,
    100% {
      border-color: color-mix(
        in srgb,
        var(--color-error) 45%,
        var(--color-base-300)
      );
      box-shadow:
        0 0 0 0 color-mix(in srgb, var(--color-error) 0%, transparent),
        0 0 0 1px color-mix(in srgb, var(--color-error) 12%, transparent);
    }
    50% {
      border-color: color-mix(
        in srgb,
        var(--color-error) 90%,
        var(--color-base-300)
      );
      box-shadow:
        0 0 0 3px color-mix(in srgb, var(--color-error) 24%, transparent),
        0 0 10px color-mix(in srgb, var(--color-error) 22%, transparent);
    }
  }

  @keyframes volume-alert-thumb-desktop {
    0%,
    100% {
      border-color: color-mix(
        in srgb,
        var(--color-error) 45%,
        var(--color-base-300)
      );
      box-shadow:
        0 0 0 0 color-mix(in srgb, var(--color-error) 0%, transparent),
        0 0 0 1px color-mix(in srgb, var(--color-error) 12%, transparent);
      transform: scale(1);
    }
    50% {
      border-color: color-mix(
        in srgb,
        var(--color-error) 90%,
        var(--color-base-300)
      );
      box-shadow:
        0 0 0 3px color-mix(in srgb, var(--color-error) 24%, transparent),
        0 0 10px color-mix(in srgb, var(--color-error) 22%, transparent);
      transform: scale(1.12);
    }
  }
</style>
