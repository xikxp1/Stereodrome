<script lang="ts">
  import { artistsQuery } from "$lib/db/collections";
  import type { Artist } from "$lib/types";

  interface Props {
    onSelect?: (artist: Artist) => void;
    selectedId?: string;
  }

  let { onSelect, selectedId }: Props = $props();

  const query = artistsQuery();
</script>

<div class="flex flex-col h-full">
  <div class="px-4 py-2 border-b border-base-300">
    <h2 class="font-semibold text-sm opacity-70">Artists</h2>
  </div>

  <div class="flex-1 overflow-auto">
    {#if query.isLoading}
      <div class="flex items-center justify-center h-32">
        <span class="loading loading-spinner loading-md"></span>
      </div>
    {:else if query.error}
      <div class="p-4 text-error text-sm">Failed to load artists</div>
    {:else if query.data && query.data.length > 0}
      <ul class="menu p-0">
        {#each query.data as artist (artist.id)}
          <li>
            <button
              class="rounded-none"
              class:active={selectedId === artist.id}
              onclick={() => onSelect?.(artist)}
            >
              <span class="truncate">{artist.name}</span>
              <span class="badge badge-sm badge-ghost"
                >{artist.album_count}</span
              >
            </button>
          </li>
        {/each}
      </ul>
    {:else}
      <div class="p-4 text-center text-sm opacity-50">
        No artists found. Sync your library to get started.
      </div>
    {/if}
  </div>
</div>
