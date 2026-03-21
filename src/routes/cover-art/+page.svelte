<script lang="ts">
  import { page } from "$app/state";
  import { Music, LoaderCircle } from "lucide-svelte";
  import { getCoverArt } from "$lib/api/commands";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { error as logError } from "@tauri-apps/plugin-log";
  import { onMount } from "svelte";

  interface CoverArtUpdateEvent {
    id: string;
    album: string;
    artist: string;
  }

  // State from URL params or event updates
  let coverArtId = $state(page.url.searchParams.get("id"));
  let album = $state(page.url.searchParams.get("album") || "Unknown Album");
  let artist = $state(page.url.searchParams.get("artist") || "Unknown Artist");

  // Fetch cover art
  let imageUrl = $state<string | null>(null);
  let isLoading = $state(true);
  let error = $state<string | null>(null);

  // Listen for cover art updates from main window
  onMount(() => {
    let unlisten: UnlistenFn | null = null;

    void (async () => {
      unlisten = await listen<CoverArtUpdateEvent>(
        "cover-art-update",
        (event) => {
          coverArtId = event.payload.id;
          album = event.payload.album || "Unknown Album";
          artist = event.payload.artist || "Unknown Artist";
        }
      );
    })();

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  });

  // Fetch cover art when ID changes
  $effect(() => {
    if (coverArtId) {
      isLoading = true;
      error = null;
      getCoverArt(coverArtId, 800)
        .then((url) => {
          imageUrl = url;
          isLoading = false;
        })
        .catch((e) => {
          logError(`Failed to fetch cover art: ${e}`);
          error = String(e);
          isLoading = false;
        });
    } else {
      isLoading = false;
    }
  });
</script>

<svelte:head>
  <title>{album} - {artist}</title>
</svelte:head>

<div
  class="flex h-screen w-screen flex-col items-center justify-center bg-base-300 p-4"
>
  {#if isLoading}
    <div class="flex flex-col items-center justify-center text-base-content/60">
      <LoaderCircle class="h-12 w-12 animate-spin" />
      <p class="mt-4">Loading cover art...</p>
    </div>
  {:else if imageUrl}
    <div
      class="relative max-h-full max-w-full overflow-hidden rounded-lg shadow-2xl"
    >
      <img
        src={imageUrl}
        alt="Album cover for {album} by {artist}"
        class="max-h-[calc(100vh-8rem)] max-w-full object-contain"
      />
    </div>
    <div class="mt-4 text-center">
      <h1 class="text-lg font-semibold text-base-content">{album}</h1>
      <p class="text-sm text-base-content/60">{artist}</p>
    </div>
  {:else}
    <div class="flex flex-col items-center justify-center text-base-content/40">
      <Music class="h-24 w-24" />
      <p class="mt-4">{error || "No cover art available"}</p>
    </div>
  {/if}
</div>
