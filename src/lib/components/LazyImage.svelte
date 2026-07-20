<script lang="ts" module>
  import { getCoverArt } from "$lib/api/commands";
  import { SvelteMap } from "svelte/reactivity";

  const coverArtUrlCache = new SvelteMap<string, string>();
  const pendingCoverArtLoads = new SvelteMap<string, Promise<string>>();

  function buildCacheKey(coverArtId: string, size: number) {
    return `${coverArtId}-${size}`;
  }

  function loadSharedCoverArt(
    coverArtId: string,
    size: number
  ): Promise<string> {
    const cacheKey = buildCacheKey(coverArtId, size);
    const cachedUrl = coverArtUrlCache.get(cacheKey);
    if (cachedUrl) {
      return Promise.resolve(cachedUrl);
    }

    const pendingLoad = pendingCoverArtLoads.get(cacheKey);
    if (pendingLoad) {
      return pendingLoad;
    }

    const nextLoad = getCoverArt(coverArtId, size)
      .then((url) => {
        coverArtUrlCache.set(cacheKey, url);
        return url;
      })
      .finally(() => {
        pendingCoverArtLoads.delete(cacheKey);
      });

    pendingCoverArtLoads.set(cacheKey, nextLoad);
    return nextLoad;
  }
</script>

<script lang="ts">
  import { Music } from "lucide-svelte";

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
  let activeCacheKey = $state<string | null>(null);

  $effect(() => {
    if (!coverArtId) {
      activeCacheKey = null;
      imageUrl = null;
      isLoading = false;
      return undefined;
    }

    const cacheKey = buildCacheKey(coverArtId, size);
    activeCacheKey = cacheKey;
    imageUrl = coverArtUrlCache.get(cacheKey) ?? null;
    isLoading = false;

    if (!element || imageUrl) {
      return undefined;
    }

    const requestedCoverArtId = coverArtId;
    const requestedSize = size;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !imageUrl && !isLoading) {
          loadImage(cacheKey, requestedCoverArtId, requestedSize);
          observer.disconnect();
        }
      },
      { rootMargin: "100px" }
    );

    observer.observe(element);

    return () => observer.disconnect();
  });

  async function loadImage(
    cacheKey: string,
    requestedCoverArtId: string,
    requestedSize: number
  ) {
    if (isLoading || activeCacheKey !== cacheKey) return;

    isLoading = true;

    try {
      const url = await loadSharedCoverArt(requestedCoverArtId, requestedSize);
      if (activeCacheKey === cacheKey) {
        imageUrl = url;
      }
    } catch {
      // Failed to load - show placeholder
    } finally {
      if (activeCacheKey === cacheKey) {
        isLoading = false;
      }
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
