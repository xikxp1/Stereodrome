<script lang="ts">
  import { getCoverArt } from "$lib/api/commands";
  import { Music } from "lucide-svelte";
  import { SvelteMap } from "svelte/reactivity";

  interface Props {
    coverArtId: string | null | undefined;
    size?: number;
    alt: string;
    class?: string;
  }

  let { coverArtId, size = 200, alt, class: className = "" }: Props = $props();

  let element: HTMLDivElement | null = $state(null);
  let imageUrl = $state<string | null>(null);
  let isLoading = $state(false);

  // Cache for loaded cover art URLs (shared across instances)
  const urlCache = new SvelteMap<string, string>();

  $effect(() => {
    if (!element || !coverArtId) return;

    const cacheKey = `${coverArtId}-${size}`;

    // Check cache first
    if (urlCache.has(cacheKey)) {
      imageUrl = urlCache.get(cacheKey)!;
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && !imageUrl && !isLoading) {
          loadImage(cacheKey);
          observer.disconnect();
        }
      },
      { rootMargin: "100px" }
    );

    observer.observe(element);

    return () => observer.disconnect();
  });

  async function loadImage(cacheKey: string) {
    if (!coverArtId || isLoading) return;

    isLoading = true;

    try {
      const url = await getCoverArt(coverArtId, size);
      urlCache.set(cacheKey, url);
      imageUrl = url;
    } catch {
      // Failed to load - show placeholder
    } finally {
      isLoading = false;
    }
  }
</script>

<div
  bind:this={element}
  class="aspect-square bg-base-300 rounded flex items-center justify-center overflow-hidden {className}"
>
  {#if imageUrl}
    <img src={imageUrl} {alt} class="w-full h-full object-cover" />
  {:else if isLoading}
    <span class="loading loading-spinner loading-sm opacity-30"></span>
  {:else}
    <Music class="h-12 w-12 opacity-30" />
  {/if}
</div>
