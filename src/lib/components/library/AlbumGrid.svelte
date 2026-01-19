<script lang="ts">
  import { albumsQuery } from '$lib/db/collections';
  import type { Album } from '$lib/types';

  interface Props {
    artistId?: string;
    onSelect?: (album: Album) => void;
    selectedId?: string;
  }

  let { artistId, onSelect, selectedId }: Props = $props();

  const query = $derived(albumsQuery(artistId));
</script>

<div class="flex flex-col h-full">
  <div class="px-4 py-2 border-b border-base-300">
    <h2 class="font-semibold text-sm opacity-70">Albums</h2>
  </div>

  <div class="flex-1 overflow-auto p-4">
    {#if $query.isLoading}
      <div class="flex items-center justify-center h-32">
        <span class="loading loading-spinner loading-md"></span>
      </div>
    {:else if $query.error}
      <div class="text-error text-sm">
        Failed to load albums
      </div>
    {:else if $query.data && $query.data.length > 0}
      <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-4">
        {#each $query.data as album (album.id)}
          <button
            class="card bg-base-200 hover:bg-base-300 transition-colors cursor-pointer text-left"
            class:ring-2={selectedId === album.id}
            class:ring-primary={selectedId === album.id}
            onclick={() => onSelect?.(album)}
          >
            <div class="card-body p-3">
              <div class="aspect-square bg-base-300 rounded mb-2 flex items-center justify-center">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 opacity-30" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 19V6l12-3v13M9 19c0 1.105-1.343 2-3 2s-3-.895-3-2 1.343-2 3-2 3 .895 3 2zm12-3c0 1.105-1.343 2-3 2s-3-.895-3-2 1.343-2 3-2 3 .895 3 2zM9 10l12-3" />
                </svg>
              </div>
              <h3 class="font-medium text-sm truncate">{album.name}</h3>
              {#if album.year}
                <p class="text-xs opacity-50">{album.year}</p>
              {/if}
              <p class="text-xs opacity-50">{album.song_count} songs</p>
            </div>
          </button>
        {/each}
      </div>
    {:else}
      <div class="text-center text-sm opacity-50 py-8">
        {artistId ? 'No albums found for this artist.' : 'Select an artist to view albums.'}
      </div>
    {/if}
  </div>
</div>
