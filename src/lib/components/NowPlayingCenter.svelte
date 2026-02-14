<script lang="ts">
  import SpectrumBars from "./SpectrumBars.svelte";
  import SyncedMarquee from "./SyncedMarquee.svelte";
  import {
    Dices,
    Music,
    Pause,
    PictureInPicture2,
    Play,
    SkipBack,
    SkipForward,
  } from "lucide-svelte";

  interface TrackInfo {
    title: string;
    artist: string;
    album?: string;
  }

  interface Props {
    mode?: "toolbar" | "mini";
    currentTrack?: TrackInfo | null;
    currentTime?: number;
    duration?: number;
    coverArtUrl?: string | null;
    isPlaying?: boolean;
    canPlayPause?: boolean;
    canPrevious?: boolean;
    canNext?: boolean;
    canReroll?: boolean;
    showNextSongInMiniPlayer?: boolean;
    showSpectrum?: boolean;
    miniControlsVisible?: boolean;
    previousTrackTooltip?: string;
    nextTrackTooltip?: string;
    rerollTrackTooltip?: string;
    nextTrack?: TrackInfo | null;
    onSeek?: (time: number) => void;
    onCoverArtClick?: () => void;
    onPlayPause?: () => void;
    onPrevious?: () => void;
    onNext?: () => void;
    onReroll?: () => void;
    onMiniPlayerToggle?: () => void;
  }

  let {
    mode = "toolbar",
    currentTrack = null,
    currentTime = 0,
    duration = 0,
    coverArtUrl = null,
    isPlaying = false,
    canPlayPause = true,
    canPrevious = true,
    canNext = true,
    canReroll = false,
    showNextSongInMiniPlayer = true,
    showSpectrum = true,
    miniControlsVisible = false,
    previousTrackTooltip = "Previous",
    nextTrackTooltip = "Next",
    rerollTrackTooltip = "Reroll next track",
    nextTrack = null,
    onSeek,
    onCoverArtClick,
    onPlayPause,
    onPrevious,
    onNext,
    onReroll,
    onMiniPlayerToggle,
  }: Props = $props();

  const isMini = $derived(mode === "mini");
  const marqueeGroup = $derived(isMini ? "mini-now-playing" : "now-playing");
  const showMiniControls = $derived(isMini && miniControlsVisible);

  function formatTime(seconds: number): string {
    if (!seconds || Number.isNaN(seconds)) return "0:00";
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  }

  function formatTimeRemaining(current: number, total: number): string {
    const remaining = total - current;
    if (!remaining || Number.isNaN(remaining)) return "-0:00";
    const mins = Math.floor(remaining / 60);
    const secs = Math.floor(remaining % 60);
    return `-${mins}:${secs.toString().padStart(2, "0")}`;
  }

  function formatArtistTitle(track: TrackInfo | null): string {
    const artist = track?.artist || "Unknown Artist";
    const title = track?.title || "Unknown Title";
    return `${artist} — ${title}`;
  }
</script>

{#if mode === "toolbar"}
  <div
    class="pointer-events-none absolute left-1/2 top-1/2 flex w-full max-w-[40%] -translate-x-1/2 -translate-y-1/2 justify-center px-4"
  >
    <div
      class="group/toolbar-nowplaying pointer-events-auto relative flex w-full items-center gap-2 overflow-hidden rounded border border-base-300 bg-base-100 p-1.5 shadow-sm"
    >
      {#if onMiniPlayerToggle}
        <div
          class="absolute right-1 top-1 z-30 flex items-center gap-1 opacity-0 transition-opacity pointer-events-none group-hover/toolbar-nowplaying:opacity-100 group-hover/toolbar-nowplaying:pointer-events-auto"
        >
          <button
            type="button"
            class="flex h-5 w-5 items-center justify-center rounded bg-base-100/85 text-base-content/70 transition-colors hover:bg-base-100 hover:text-base-content"
            onclick={() => onMiniPlayerToggle()}
            aria-label="Toggle mini player"
            title="Mini Player"
          >
            <PictureInPicture2 class="h-3 w-3" />
          </button>
        </div>
      {/if}

      {#if showSpectrum}
        <SpectrumBars />
      {/if}

      <button
        class="relative z-10 flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200 transition-opacity hover:opacity-80"
        onclick={() => onCoverArtClick?.()}
        disabled={!coverArtUrl}
        aria-label="View cover art"
      >
        {#if coverArtUrl}
          <img
            src={coverArtUrl}
            alt="Cover art"
            class="h-full w-full object-cover"
          />
        {:else}
          <Music class="h-6 w-6 text-base-content/30" />
        {/if}
      </button>

      <div class="relative z-10 min-w-0 flex-1">
        <div class="mb-1 text-center">
          {#if currentTrack}
            <SyncedMarquee
              text={currentTrack.title}
              group={marqueeGroup}
              class="text-sm font-medium text-base-content"
            />
            <SyncedMarquee
              text={currentTrack.artist +
                (currentTrack.album ? ` — ${currentTrack.album}` : "")}
              group={marqueeGroup}
              class="text-xs text-base-content/60"
            />
          {:else}
            <div class="text-sm font-medium text-base-content">&nbsp;</div>
            <div class="text-xs text-base-content/40">Not Playing</div>
          {/if}
        </div>

        <div class="flex items-center gap-2">
          <span
            class="w-9 text-right font-mono text-[11px] tabular-nums text-base-content"
          >
            {formatTime(currentTime)}
          </span>
          <div class="relative flex h-3 flex-1 items-center">
            <div class="absolute h-1 w-full rounded-full bg-base-300"></div>
            <div
              class="absolute h-1 rounded-full bg-primary"
              style="width: {duration > 0
                ? (currentTime / duration) * 100
                : 0}%"
            ></div>
            <input
              type="range"
              min="0"
              max={duration || 100}
              value={currentTime}
              class="absolute w-full cursor-pointer opacity-0"
              oninput={(e) => onSeek?.(Number(e.currentTarget.value))}
              aria-label="Seek"
            />
            <div
              class="pointer-events-none absolute h-2.5 w-2.5 rounded-full border border-primary/50 bg-base-100 shadow-sm"
              style="left: calc({duration > 0
                ? (currentTime / duration) * 100
                : 0}% - 5px)"
            ></div>
          </div>
          <span
            class="w-9 font-mono text-[11px] tabular-nums text-base-content"
          >
            {formatTimeRemaining(currentTime, duration)}
          </span>
        </div>
      </div>
    </div>
  </div>
{:else}
  <div
    class="group/nowplaying relative flex w-full items-center gap-1.5 overflow-hidden rounded bg-transparent px-0.5 py-0"
  >
    <div
      class="relative z-10 flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200"
    >
      {#if coverArtUrl}
        <img
          src={coverArtUrl}
          alt="Cover art"
          class="h-full w-full object-cover"
        />
      {:else}
        <Music class="h-6 w-6 text-base-content/30" />
      {/if}

      <button
        type="button"
        class="absolute inset-0 z-20 flex items-center justify-center bg-base-content/35 text-base-100 opacity-0 transition-all hover:bg-base-content/55 active:bg-base-content/60"
        class:opacity-100={showMiniControls}
        class:pointer-events-none={!showMiniControls}
        onclick={(event) => {
          event.stopPropagation();
          onPlayPause?.();
        }}
        disabled={!canPlayPause}
        aria-label={isPlaying ? "Pause" : "Play"}
        title={isPlaying ? "Pause" : "Play"}
      >
        {#if isPlaying}
          <Pause class="h-4 w-4" fill="currentColor" />
        {:else}
          <Play class="ml-0.5 h-4 w-4" fill="currentColor" />
        {/if}
      </button>
    </div>

    <div class="min-w-0 flex-1">
      <div class="mb-1 text-center">
        {#if isMini}
          {#if showNextSongInMiniPlayer}
            <SyncedMarquee
              text={formatArtistTitle(currentTrack)}
              group={marqueeGroup}
              class="text-sm font-medium text-base-content"
            />
            <SyncedMarquee
              text="Next: {formatArtistTitle(nextTrack)}"
              group={marqueeGroup}
              class="text-xs text-base-content/60"
            />
          {:else if currentTrack}
            <SyncedMarquee
              text={currentTrack.title}
              group={marqueeGroup}
              class="text-sm font-medium text-base-content"
            />
            <SyncedMarquee
              text={currentTrack.artist +
                (currentTrack.album ? ` — ${currentTrack.album}` : "")}
              group={marqueeGroup}
              class="text-xs text-base-content/60"
            />
          {:else}
            <div class="text-sm font-medium text-base-content">&nbsp;</div>
            <div class="text-xs text-base-content/40">Not Playing</div>
          {/if}
        {:else if currentTrack}
          <SyncedMarquee
            text={currentTrack.title}
            group={marqueeGroup}
            class="text-sm font-medium text-base-content"
          />
          <SyncedMarquee
            text={currentTrack.artist +
              (currentTrack.album ? ` — ${currentTrack.album}` : "")}
            group={marqueeGroup}
            class="text-xs text-base-content/60"
          />
        {:else}
          <div class="text-sm font-medium text-base-content">&nbsp;</div>
          <div class="text-xs text-base-content/40">Not Playing</div>
        {/if}
      </div>

      <div class="flex items-center gap-2">
        <div class="relative w-7">
          <span
            class="block text-right font-mono text-[11px] tabular-nums text-base-content transition-opacity"
            class:opacity-0={showMiniControls}
          >
            {formatTime(currentTime)}
          </span>
          <button
            type="button"
            class="absolute inset-0 flex items-center justify-end rounded pr-1 opacity-0 transition-all hover:bg-base-200/70 hover:text-primary active:bg-base-200/85 disabled:opacity-30"
            class:opacity-100={showMiniControls}
            class:pointer-events-none={!showMiniControls}
            onclick={() => onPrevious?.()}
            disabled={!canPrevious}
            aria-label="Previous track"
            title={previousTrackTooltip}
          >
            <SkipBack
              class="h-3.5 w-3.5 text-base-content/80"
              fill="currentColor"
            />
          </button>
        </div>

        <div class="relative flex h-3 flex-1 items-center">
          <div class="absolute h-1 w-full rounded-full bg-base-300"></div>
          <div
            class="absolute h-1 rounded-full bg-primary"
            style="width: {duration > 0 ? (currentTime / duration) * 100 : 0}%"
          ></div>
          <input
            type="range"
            min="0"
            max={duration || 100}
            value={currentTime}
            class="absolute w-full cursor-pointer opacity-0"
            oninput={(e) => onSeek?.(Number(e.currentTarget.value))}
            aria-label="Seek"
          />
          <div
            class="pointer-events-none absolute h-2.5 w-2.5 rounded-full border border-primary/50 bg-base-100 shadow-sm"
            style="left: calc({duration > 0
              ? (currentTime / duration) * 100
              : 0}% - 5px)"
          ></div>
        </div>

        <div class="relative w-7">
          <span
            class="block font-mono text-[11px] tabular-nums text-base-content transition-opacity"
            class:opacity-0={showMiniControls}
          >
            {formatTimeRemaining(currentTime, duration)}
          </span>
          <button
            type="button"
            class="absolute inset-0 flex items-center justify-start rounded pl-1 opacity-0 transition-all hover:bg-base-200/70 hover:text-primary active:bg-base-200/85 disabled:opacity-30"
            class:opacity-100={showMiniControls}
            class:pointer-events-none={!showMiniControls}
            onclick={() => onNext?.()}
            disabled={!canNext}
            aria-label="Next track"
            title={nextTrackTooltip}
          >
            <SkipForward
              class="h-3.5 w-3.5 text-base-content/80"
              fill="currentColor"
            />
          </button>
        </div>

        <div class="relative w-5">
          <button
            type="button"
            class="absolute inset-0 flex items-center justify-center rounded opacity-0 transition-all hover:bg-base-200/70 hover:text-primary active:bg-base-200/85 disabled:opacity-30"
            class:opacity-100={showMiniControls}
            class:pointer-events-none={!showMiniControls}
            onclick={() => onReroll?.()}
            disabled={!canReroll}
            aria-label="Reroll next track"
            title={rerollTrackTooltip}
          >
            <Dices class="h-3 w-3 text-base-content/80" />
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}
