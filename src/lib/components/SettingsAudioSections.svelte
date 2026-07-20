<script lang="ts">
  import {
    Disc3,
    FolderOpen,
    HardDrive,
    RefreshCw,
    RotateCcw,
    SlidersHorizontal,
    Trash2,
    Volume2,
  } from "lucide-svelte";
  import type { CacheStats } from "$lib/api/commands";
  import type {
    AnalysisProgress,
    CacheLocationInfo,
    CacheRootUpdateResult,
    NormalizationSettings,
    NormalizationStats,
    PlaybackSettings,
  } from "$lib/types";
  import {
    EQ_BAND_LABELS,
    EQ_MAX_DB,
    EQ_MIN_DB,
    binauralDescriptions,
    binauralPresets,
    dynamicsDescriptions,
    dynamicsPresets,
    eqPresets,
    lufsPresets,
    sizePresets,
    type EqPresetId,
  } from "./settingsAudio";

  interface Props {
    playbackSettings: PlaybackSettings | null;
    handlePlaybackSettingChange: (
      update: Partial<PlaybackSettings>
    ) => void | Promise<void>;
    activeEqPreset: EqPresetId | null;
    activeEqDescription: string;
    applyEqPreset: (presetId: EqPresetId) => void | Promise<void>;
    formatDb: (value: number) => string;
    getEqBandValue: (index: number) => number;
    previewEqBand: (index: number, value: number) => void;
    commitEqBand: (index: number, value: number) => void | Promise<void>;
    loadingNorm: boolean;
    normSettings: NormalizationSettings | null;
    handleNormSettingChange: (
      update: Partial<NormalizationSettings>
    ) => void | Promise<void>;
    savingNorm: boolean;
    normStats: NormalizationStats | null;
    analyzing: boolean;
    analysisProgress: AnalysisProgress | null;
    handleAnalyzeAll: () => void | Promise<void>;
    clearingNorm: boolean;
    handleClearNormData: () => void | Promise<void>;
    loadingStats: boolean;
    cacheStats: CacheStats | null;
    loadingCacheLocations: boolean;
    movingCacheLocation: boolean;
    cacheLocations: CacheLocationInfo | null;
    handleChooseCacheRoot: () => void | Promise<void>;
    handleResetCacheRoot: () => void | Promise<void>;
    cacheMoveResult: CacheRootUpdateResult | null;
    cacheSizeGB: number;
    savingSize: boolean;
    handleSizeChange: (sizeGB: number) => void | Promise<void>;
    loadCacheStats: () => void | Promise<void>;
    clearing: boolean;
    handleClearCache: () => void | Promise<void>;
  }

  let {
    playbackSettings,
    handlePlaybackSettingChange,
    activeEqPreset,
    activeEqDescription,
    applyEqPreset,
    formatDb,
    getEqBandValue,
    previewEqBand,
    commitEqBand,
    loadingNorm,
    normSettings,
    handleNormSettingChange,
    savingNorm,
    normStats,
    analyzing,
    analysisProgress,
    handleAnalyzeAll,
    clearingNorm,
    handleClearNormData,
    loadingStats,
    cacheStats,
    loadingCacheLocations,
    movingCacheLocation,
    cacheLocations,
    handleChooseCacheRoot,
    handleResetCacheRoot,
    cacheMoveResult,
    cacheSizeGB,
    savingSize,
    handleSizeChange,
    loadCacheStats,
    clearing,
    handleClearCache,
  }: Props = $props();

  function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${(bytes / k ** i).toFixed(1)} ${sizes[i]}`;
  }

  function formatMoveResult(result: CacheRootUpdateResult): string {
    const moved = result.audio.moved_files + result.cover_art.moved_files;
    const skipped = result.audio.skipped_files + result.cover_art.skipped_files;
    const failed = result.audio.failed_files + result.cover_art.failed_files;
    const parts = [`${moved} moved`];
    if (skipped > 0) parts.push(`${skipped} skipped`);
    if (failed > 0) parts.push(`${failed} failed`);
    return parts.join(", ");
  }
</script>

<!-- Playback Section -->
<div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
  <div class="mb-3 flex items-center gap-2">
    <Disc3 class="h-4 w-4 text-base-content/60" />
    <h3 class="font-medium">Playback</h3>
  </div>

  {#if playbackSettings}
    <div class="space-y-3">
      <label class="flex cursor-pointer items-center justify-between">
        <span class="text-sm">Gapless playback</span>
        <input
          type="checkbox"
          class="checkbox checkbox-sm checkbox-primary"
          checked={playbackSettings.gapless_enabled}
          onchange={(e) =>
            handlePlaybackSettingChange({
              gapless_enabled: e.currentTarget.checked,
            })}
        />
      </label>
      <p class="text-xs text-base-content/50">
        Seamless transitions between consecutive album tracks.
      </p>

      <div>
        <div class="mb-2 flex items-center justify-between text-sm">
          <span class="text-base-content/60">Files to prefetch</span>
          <span class="text-xs text-base-content/50"
            >{playbackSettings.prefetch_count}</span
          >
        </div>
        <div class="flex flex-wrap gap-1">
          {#each [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as count (count)}
            <button
              type="button"
              class="btn btn-xs h-6 min-h-0 px-2 {playbackSettings.prefetch_count ===
              count
                ? 'btn-primary'
                : 'btn-ghost'}"
              onclick={() =>
                handlePlaybackSettingChange({ prefetch_count: count })}
            >
              {count}
            </button>
          {/each}
        </div>
        <p class="mt-2 text-xs text-base-content/50">
          Download this many upcoming uncached songs in the background.
        </p>
      </div>

      <div class="border-t border-base-300 pt-3"></div>

      <label class="flex cursor-pointer items-center justify-between">
        <span class="text-sm">Crossfade</span>
        <input
          type="checkbox"
          class="checkbox checkbox-sm checkbox-primary"
          checked={playbackSettings.crossfade_enabled}
          onchange={(e) =>
            handlePlaybackSettingChange({
              crossfade_enabled: e.currentTarget.checked,
            })}
        />
      </label>
      <p class="text-xs text-base-content/50">
        Smooth volume transitions between non-album tracks.
      </p>

      {#if playbackSettings.crossfade_enabled}
        <div>
          <div class="mb-2 flex items-center justify-between text-sm">
            <span class="text-base-content/60">Duration</span>
            <span class="text-xs text-base-content/50"
              >{(playbackSettings.crossfade_duration_ms / 1000).toFixed(
                0
              )}s</span
            >
          </div>
          <div class="flex flex-wrap gap-1">
            {#each [1000, 3000, 5000, 8000, 12000] as duration (duration)}
              <button
                type="button"
                class="btn btn-xs h-6 min-h-0 px-2 {playbackSettings.crossfade_duration_ms ===
                duration
                  ? 'btn-primary'
                  : 'btn-ghost'}"
                onclick={() =>
                  handlePlaybackSettingChange({
                    crossfade_duration_ms: duration,
                  })}
              >
                {duration / 1000}s
              </button>
            {/each}
          </div>
        </div>

        <label class="flex cursor-pointer items-center justify-between">
          <span class="text-sm">Manual Next crossfade</span>
          <input
            type="checkbox"
            class="checkbox checkbox-sm checkbox-primary"
            checked={playbackSettings.crossfade_on_manual_queue_advance}
            onchange={(e) =>
              handlePlaybackSettingChange({
                crossfade_on_manual_queue_advance: e.currentTarget.checked,
              })}
          />
        </label>
        <p class="text-xs text-base-content/50">
          Apply crossfade when you manually advance the queue with Next.
        </p>
      {/if}

      <div class="border-t border-base-300 pt-3"></div>

      <label class="flex cursor-pointer items-center justify-between">
        <span class="text-sm">Binaural crossfeed</span>
        <input
          type="checkbox"
          class="checkbox checkbox-sm checkbox-primary"
          checked={playbackSettings.binaural_enabled}
          onchange={(e) =>
            handlePlaybackSettingChange({
              binaural_enabled: e.currentTarget.checked,
            })}
        />
      </label>
      <p class="text-xs text-base-content/50">
        Convert stereo to binaural-style headphone playback with bs2b.
      </p>

      {#if playbackSettings.binaural_enabled}
        <div>
          <div class="mb-2 text-sm text-base-content/60">Preset</div>
          <div class="flex flex-wrap gap-1">
            {#each binauralPresets as preset (preset)}
              <button
                type="button"
                class="btn btn-xs h-6 min-h-0 px-3 {playbackSettings.binaural_preset ===
                preset
                  ? 'btn-primary'
                  : 'btn-ghost'}"
                onclick={() =>
                  handlePlaybackSettingChange({
                    binaural_preset: preset,
                  })}
              >
                {preset === "default"
                  ? "Default"
                  : preset === "cmoy"
                    ? "C-Moy"
                    : preset === "jmeier"
                      ? "JMeier"
                      : "Aggressive"}
              </button>
            {/each}
          </div>
          <p class="mt-1 text-xs text-base-content/50">
            {binauralDescriptions[playbackSettings.binaural_preset]}
          </p>
        </div>
      {/if}

      <div class="border-t border-base-300 pt-3"></div>

      <div
        class="eq-card rounded-md border border-base-300/80 bg-base-100/60 p-3"
      >
        <div class="mb-3 flex items-center justify-between">
          <div class="flex items-center gap-2">
            <SlidersHorizontal class="h-4 w-4 text-primary" />
            <span class="text-sm font-medium">Equalizer</span>
          </div>
          <input
            type="checkbox"
            class="checkbox checkbox-sm checkbox-primary"
            checked={playbackSettings.equalizer_enabled}
            onchange={(e) =>
              handlePlaybackSettingChange({
                equalizer_enabled: e.currentTarget.checked,
              })}
          />
        </div>

        <div class="mb-2 flex flex-wrap gap-1">
          {#each eqPresets as preset (preset.id)}
            <button
              type="button"
              class="btn btn-xs h-6 min-h-0 px-2 {activeEqPreset === preset.id
                ? 'btn-primary'
                : 'btn-ghost'}"
              onclick={() => applyEqPreset(preset.id)}
            >
              {preset.label}
            </button>
          {/each}
        </div>
        <p class="mb-3 text-xs text-base-content/55">
          {activeEqDescription}
        </p>

        <div
          class="eq-grid rounded border border-base-300/70 bg-linear-to-b from-base-200/60 to-base-300/20 p-2"
          class:opacity-60={!playbackSettings.equalizer_enabled}
        >
          {#each EQ_BAND_LABELS as label, index (`eq-band-${label}`)}
            <div class="eq-band">
              <div class="eq-band-value">
                {formatDb(getEqBandValue(index))}
              </div>
              <div class="eq-slider-wrap">
                <input
                  type="range"
                  min={EQ_MIN_DB}
                  max={EQ_MAX_DB}
                  step="0.5"
                  class="eq-slider"
                  value={getEqBandValue(index)}
                  oninput={(e) =>
                    previewEqBand(index, parseFloat(e.currentTarget.value))}
                  onchange={(e) =>
                    commitEqBand(index, parseFloat(e.currentTarget.value))}
                />
              </div>
              <div class="eq-band-label">{label}</div>
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>

<!-- Volume Normalization Section -->
<div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
  <div class="mb-3 flex items-center gap-2">
    <Volume2 class="h-4 w-4 text-base-content/60" />
    <h3 class="font-medium">Volume Normalization</h3>
  </div>

  {#if loadingNorm}
    <div class="flex items-center gap-2 text-sm text-base-content/60">
      <RefreshCw class="h-4 w-4 animate-spin" />
      Loading...
    </div>
  {:else if normSettings}
    <div class="space-y-3">
      <!-- Enable toggle -->
      <label class="flex cursor-pointer items-center justify-between">
        <span class="text-sm">Enable</span>
        <input
          type="checkbox"
          class="checkbox checkbox-sm checkbox-primary"
          checked={normSettings.enabled}
          onchange={(e) =>
            handleNormSettingChange({
              enabled: e.currentTarget.checked,
            })}
        />
      </label>

      {#if normSettings.enabled}
        <!-- Mode selection -->
        <div>
          <div class="mb-1 text-sm text-base-content/60">Mode</div>
          <div class="flex gap-3">
            <label class="flex cursor-pointer items-center gap-1.5">
              <input
                type="radio"
                name="norm-mode"
                class="radio radio-sm radio-primary"
                checked={normSettings.mode === "track"}
                onchange={() => handleNormSettingChange({ mode: "track" })}
              />
              <span class="text-sm">Track</span>
            </label>
            <label class="flex cursor-pointer items-center gap-1.5">
              <input
                type="radio"
                name="norm-mode"
                class="radio radio-sm radio-primary"
                checked={normSettings.mode === "album"}
                onchange={() => handleNormSettingChange({ mode: "album" })}
              />
              <span class="text-sm">Album</span>
            </label>
          </div>
          <p class="mt-1 text-xs text-base-content/50">
            {normSettings.mode === "album"
              ? "Preserves relative dynamics within albums."
              : "Each track normalized independently."}
          </p>
        </div>

        <!-- Target LUFS presets -->
        <div>
          <div class="mb-2 flex items-center justify-between text-sm">
            <span class="text-base-content/60">Target level</span>
            <span class="text-xs text-base-content/50"
              >{normSettings.target_lufs} LUFS</span
            >
          </div>
          <div class="flex flex-wrap gap-1">
            {#each lufsPresets as preset (preset)}
              <button
                type="button"
                class="btn btn-xs h-6 min-h-0 px-2 {normSettings.target_lufs ===
                preset
                  ? 'btn-primary'
                  : 'btn-ghost'}"
                onclick={() => handleNormSettingChange({ target_lufs: preset })}
                disabled={savingNorm}
              >
                {preset}
              </button>
            {/each}
          </div>
        </div>

        <!-- Pre-amp slider -->
        <div>
          <div class="mb-1 flex items-center justify-between text-sm">
            <span class="text-base-content/60">Pre-amp</span>
            <span class="text-xs text-base-content/50"
              >{normSettings.pre_amp_db > 0
                ? "+"
                : ""}{normSettings.pre_amp_db.toFixed(1)} dB</span
            >
          </div>
          <input
            type="range"
            min="-6"
            max="6"
            step="0.5"
            class="preamp-slider w-full"
            value={normSettings.pre_amp_db}
            oninput={(e) => {
              if (normSettings)
                normSettings.pre_amp_db = parseFloat(e.currentTarget.value);
            }}
            onchange={(e) =>
              handleNormSettingChange({
                pre_amp_db: parseFloat(e.currentTarget.value),
              })}
          />
        </div>

        <!-- Prevent clipping -->
        <label class="flex cursor-pointer items-center justify-between">
          <span class="text-sm">Prevent clipping</span>
          <input
            type="checkbox"
            class="checkbox checkbox-sm checkbox-primary"
            checked={normSettings.prevent_clipping}
            onchange={(e) =>
              handleNormSettingChange({
                prevent_clipping: e.currentTarget.checked,
              })}
          />
        </label>

        <!-- Dynamics processing -->
        <label class="flex cursor-pointer items-center justify-between">
          <span class="text-sm">Dynamics processing</span>
          <input
            type="checkbox"
            class="checkbox checkbox-sm checkbox-primary"
            checked={normSettings.dynamics_enabled}
            onchange={(e) =>
              handleNormSettingChange({
                dynamics_enabled: e.currentTarget.checked,
              })}
          />
        </label>

        {#if normSettings.dynamics_enabled}
          <div>
            <div class="mb-2 text-sm text-base-content/60">Amount</div>
            <div class="flex flex-wrap gap-1">
              {#each dynamicsPresets as preset (preset)}
                <button
                  type="button"
                  class="btn btn-xs h-6 min-h-0 px-3 {normSettings.dynamics_preset ===
                  preset
                    ? 'btn-primary'
                    : 'btn-ghost'}"
                  onclick={() =>
                    handleNormSettingChange({
                      dynamics_preset: preset,
                    })}
                  disabled={savingNorm}
                >
                  {preset.charAt(0).toUpperCase() + preset.slice(1)}
                </button>
              {/each}
            </div>
            <p class="mt-1 text-xs text-base-content/50">
              {dynamicsDescriptions[normSettings.dynamics_preset]}
            </p>
          </div>
        {/if}
      {/if}

      <!-- Analysis stats -->
      {#if normStats}
        <div class="border-t border-base-300 pt-3">
          <div class="flex justify-between text-sm">
            <span class="text-base-content/60">Songs analyzed</span>
            <span class="font-medium">
              {normStats.analyzed_count.toLocaleString()} / {normStats.total_count.toLocaleString()}
            </span>
          </div>

          <!-- Progress bar -->
          <div class="mt-2">
            <div class="h-2 w-full overflow-hidden rounded-full bg-base-300">
              <div
                class="h-full bg-primary transition-all"
                style="width: {normStats.total_count > 0
                  ? Math.min(
                      100,
                      (normStats.analyzed_count / normStats.total_count) * 100
                    )
                  : 0}%"
              ></div>
            </div>
            {#if analyzing && analysisProgress}
              <div class="mt-1 text-right text-xs text-base-content/50">
                Analyzing... {analysisProgress.analyzed} / {analysisProgress.total}
              </div>
            {:else}
              <div class="mt-1 text-right text-xs text-base-content/50">
                {normStats.total_count > 0
                  ? (
                      (normStats.analyzed_count / normStats.total_count) *
                      100
                    ).toFixed(1)
                  : "0.0"}% analyzed
              </div>
            {/if}
          </div>

          <div class="mt-3 flex gap-2">
            <button
              type="button"
              class="btn btn-sm btn-ghost gap-1"
              onclick={handleAnalyzeAll}
              disabled={analyzing ||
                normStats.analyzed_count >= normStats.total_count}
            >
              {#if analyzing}
                <RefreshCw class="h-3.5 w-3.5 animate-spin" />
                Analyzing...
              {:else}
                <RefreshCw class="h-3.5 w-3.5" />
                Analyze All
              {/if}
            </button>
            <button
              type="button"
              class="btn btn-sm btn-error btn-outline gap-1"
              onclick={handleClearNormData}
              disabled={clearingNorm || normStats.analyzed_count === 0}
            >
              <Trash2 class="h-3.5 w-3.5" />
              {clearingNorm ? "Clearing..." : "Clear Data"}
            </button>
          </div>
        </div>
      {/if}
    </div>
  {:else}
    <div class="text-sm text-base-content/60">
      Unable to load normalization settings
    </div>
  {/if}
</div>

<!-- Audio Cache Section -->
<div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
  <div class="mb-3 flex items-center gap-2">
    <HardDrive class="h-4 w-4 text-base-content/60" />
    <h3 class="font-medium">Audio Cache</h3>
  </div>

  {#if loadingStats}
    <div class="flex items-center gap-2 text-sm text-base-content/60">
      <RefreshCw class="h-4 w-4 animate-spin" />
      Loading...
    </div>
  {:else if cacheStats}
    <div class="mb-4 space-y-2">
      <div class="border-b border-base-300 pb-3">
        <div class="mb-2 flex items-center justify-between text-sm">
          <span class="text-base-content/60">Cache folder</span>
          {#if loadingCacheLocations || movingCacheLocation}
            <RefreshCw class="h-3.5 w-3.5 animate-spin" />
          {/if}
        </div>
        {#if cacheLocations}
          <div
            class="break-all rounded-md bg-base-300/60 px-2 py-1.5 font-mono text-xs text-base-content/70"
            title={cacheLocations.cache_root}
          >
            {cacheLocations.cache_root}
          </div>
          <div class="mt-2 flex flex-wrap gap-2">
            <button
              type="button"
              class="btn btn-xs h-7 min-h-0 gap-1"
              onclick={handleChooseCacheRoot}
              disabled={movingCacheLocation}
            >
              <FolderOpen class="h-3.5 w-3.5" />
              Choose Folder
            </button>
            <button
              type="button"
              class="btn btn-xs btn-ghost h-7 min-h-0 gap-1"
              onclick={handleResetCacheRoot}
              disabled={movingCacheLocation || cacheLocations.is_default}
            >
              <RotateCcw class="h-3.5 w-3.5" />
              Reset
            </button>
          </div>
          {#if cacheMoveResult}
            <p class="mt-2 text-xs text-base-content/50">
              Cache location updated: {formatMoveResult(cacheMoveResult)}.
            </p>
          {/if}
        {:else}
          <div class="text-sm text-base-content/60">
            Unable to load cache folder
          </div>
        {/if}
      </div>

      <div class="flex justify-between text-sm">
        <span class="text-base-content/60">Cached files</span>
        <span class="font-medium">{cacheStats.file_count}</span>
      </div>
      <div class="flex justify-between text-sm">
        <span class="text-base-content/60">Used space</span>
        <span class="font-medium">{formatBytes(cacheStats.total_size)}</span>
      </div>
      <div class="flex justify-between text-sm">
        <span class="text-base-content/60">Maximum size</span>
        <span class="font-medium">{formatBytes(cacheStats.max_size)}</span>
      </div>
      <!-- Progress bar -->
      <div class="mt-2">
        <div class="h-2 w-full overflow-hidden rounded-full bg-base-300">
          <div
            class="h-full bg-primary transition-all"
            style="width: {Math.min(
              100,
              (cacheStats.total_size / cacheStats.max_size) * 100
            )}%"
          ></div>
        </div>
        <div class="mt-1 text-right text-xs text-base-content/50">
          {((cacheStats.total_size / cacheStats.max_size) * 100).toFixed(1)}%
          used
        </div>
      </div>

      <!-- Cache size presets -->
      <div class="mt-4 border-t border-base-300 pt-4">
        <div class="mb-2 flex items-center justify-between text-sm">
          <span class="text-base-content/60">Maximum cache size</span>
          {#if savingSize}
            <RefreshCw class="h-3.5 w-3.5 animate-spin" />
          {/if}
        </div>
        <div class="flex flex-wrap gap-1">
          {#each sizePresets as preset (preset)}
            <button
              type="button"
              class="btn btn-xs h-6 min-h-0 px-2 {Math.abs(
                cacheSizeGB - preset
              ) < 0.01
                ? 'btn-primary'
                : 'btn-ghost'}"
              onclick={() => handleSizeChange(preset)}
              disabled={savingSize}
            >
              {preset < 1 ? `${preset * 1000}MB` : `${preset}GB`}
            </button>
          {/each}
        </div>
      </div>
    </div>

    <div class="flex gap-2">
      <button
        type="button"
        class="btn btn-sm btn-ghost gap-1"
        onclick={loadCacheStats}
        disabled={loadingStats}
      >
        <RefreshCw class="h-3.5 w-3.5 {loadingStats ? 'animate-spin' : ''}" />
        Refresh
      </button>
      <button
        type="button"
        class="btn btn-sm btn-error btn-outline gap-1"
        onclick={handleClearCache}
        disabled={clearing || cacheStats.file_count === 0}
      >
        <Trash2 class="h-3.5 w-3.5" />
        {clearing ? "Clearing..." : "Clear Cache"}
      </button>
    </div>
  {:else}
    <div class="text-sm text-base-content/60">
      Unable to load cache statistics
    </div>
  {/if}
</div>

<style>
  .eq-card {
    box-shadow:
      inset 0 1px 0 oklch(1 0 0 / 0.7),
      0 8px 20px oklch(0.42 0.02 250 / 0.08);
  }

  .eq-grid {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: 10px;
  }

  .eq-band {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .eq-band-value {
    font-size: 0.65rem;
    line-height: 1;
    color: oklch(0.48 0.02 250);
    min-height: 0.75rem;
    font-variant-numeric: tabular-nums;
  }

  .eq-band-label {
    font-size: 0.65rem;
    line-height: 1;
    color: oklch(0.42 0.01 250);
    letter-spacing: 0.01em;
  }

  .eq-slider-wrap {
    height: 92px;
    width: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .eq-slider {
    -webkit-appearance: none;
    appearance: none;
    width: 92px;
    height: 4px;
    border-radius: 999px;
    background: linear-gradient(
      to right,
      oklch(0.72 0.05 245 / 0.9),
      oklch(0.45 0.12 250 / 0.9)
    );
    transform: rotate(-90deg);
    cursor: ns-resize;
    outline: none;
  }

  .eq-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 13px;
    height: 13px;
    border-radius: 50%;
    background: oklch(0.97 0.01 250);
    border: 1px solid oklch(0.52 0.04 250);
    box-shadow: 0 1px 3px oklch(0 0 0 / 0.28);
  }

  .eq-slider:focus-visible::-webkit-slider-thumb {
    outline: 2px solid oklch(0.58 0.2 250);
    outline-offset: 2px;
  }

  .preamp-slider {
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    border-radius: 2px;
    background: oklch(0.3 0 0);
    outline: none;
    cursor: pointer;
  }

  .preamp-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: oklch(0.58 0.2 250);
    cursor: pointer;
  }

  @media (min-width: 540px) {
    .eq-grid {
      grid-template-columns: repeat(12, minmax(0, 1fr));
    }
  }
</style>
