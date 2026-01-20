<script lang="ts">
  import { spectrum } from "$lib/stores/spectrum.svelte";
  import { playback } from "$lib/stores/playback.svelte";

  // Store last active bands to keep shape during fade out
  let lastActiveBands = $state([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

  // Update last active bands only when playing
  $effect(() => {
    if (playback.isPlaying && spectrum.bands.some((b) => b > 0.05)) {
      lastActiveBands = [...spectrum.bands];
    }
  });

  // Use current bands when playing, last bands when fading out
  let displayBands = $derived(
    playback.isPlaying ? spectrum.bands : lastActiveBands
  );

  // Generate smooth SVG path from band values
  function generatePath(
    bands: number[],
    width: number,
    height: number
  ): string {
    if (bands.length === 0) return "";

    const points: [number, number][] = [];

    // Generate points for each band across full width
    for (let i = 0; i < bands.length; i++) {
      const x = (i / (bands.length - 1)) * width;
      const y = height - bands[i] * height * 0.85;
      points.push([x, Math.max(y, height * 0.1)]);
    }

    // Create smooth bezier curve through points
    let path = `M 0 ${height}`;
    path += ` L ${points[0][0]} ${points[0][1]}`;

    for (let i = 0; i < points.length - 1; i++) {
      const [x1, y1] = points[i];
      const [x2, y2] = points[i + 1];
      const cpx = (x1 + x2) / 2;
      path += ` Q ${x1} ${y1} ${cpx} ${(y1 + y2) / 2}`;
    }

    // Final point and close
    const last = points[points.length - 1];
    path += ` Q ${last[0]} ${last[1]} ${last[0]} ${last[1]}`;
    path += ` L ${width} ${height}`;
    path += " Z";

    return path;
  }

  let pathData = $derived(generatePath(displayBands, 400, 100));
</script>

{#if spectrum.enabled}
  <div class="spectrum-backdrop" class:active={playback.isPlaying}>
    <svg viewBox="0 0 400 100" preserveAspectRatio="none" class="spectrum-svg">
      <defs>
        <linearGradient id="spectrumGradient" x1="0%" y1="100%" x2="0%" y2="0%">
          <stop
            offset="0%"
            stop-color="oklch(65% 0.15 250)"
            stop-opacity="0.4"
          />
          <stop
            offset="40%"
            stop-color="oklch(70% 0.12 260)"
            stop-opacity="0.25"
          />
          <stop
            offset="80%"
            stop-color="oklch(75% 0.08 270)"
            stop-opacity="0.1"
          />
          <stop
            offset="100%"
            stop-color="oklch(80% 0.05 280)"
            stop-opacity="0"
          />
        </linearGradient>
      </defs>
      <path d={pathData} fill="url(#spectrumGradient)" class="spectrum-path" />
    </svg>
  </div>
{/if}

<style>
  .spectrum-backdrop {
    position: absolute;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    border-radius: inherit;
    pointer-events: none;
    opacity: 0;
    transition: opacity 1s ease-out;
  }

  .spectrum-backdrop.active {
    opacity: 1;
    transition: opacity 200ms ease-out;
  }

  .spectrum-svg {
    width: 100%;
    height: 100%;
  }

  .spectrum-path {
    transition: d 80ms ease-out;
  }
</style>
