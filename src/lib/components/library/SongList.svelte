<script lang="ts">
  import { songsQuery } from "$lib/db/collections";
  import type { Song } from "$lib/types";

  interface Props {
    albumId?: string;
    onPlay?: (song: Song) => void;
  }

  let { albumId, onPlay }: Props = $props();

  const query = $derived(songsQuery(albumId));

  function formatDuration(seconds: number | null): string {
    if (!seconds) return "--:--";
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  }
</script>

<div class="flex flex-col h-full">
  <div class="px-4 py-2 border-b border-base-300">
    <h2 class="font-semibold text-sm opacity-70">Songs</h2>
  </div>

  <div class="flex-1 overflow-auto">
    {#if $query.isLoading}
      <div class="flex items-center justify-center h-32">
        <span class="loading loading-spinner loading-md"></span>
      </div>
    {:else if $query.error}
      <div class="p-4 text-error text-sm">Failed to load songs</div>
    {:else if $query.data && $query.data.length > 0}
      <table class="table table-sm">
        <thead>
          <tr>
            <th class="w-12">#</th>
            <th>Title</th>
            <th class="w-20 text-right">Duration</th>
          </tr>
        </thead>
        <tbody>
          {#each $query.data as song (song.id)}
            <tr class="hover cursor-pointer" ondblclick={() => onPlay?.(song)}>
              <td class="opacity-50">{song.track_number || "-"}</td>
              <td class="font-medium">{song.title}</td>
              <td class="text-right opacity-50"
                >{formatDuration(song.duration)}</td
              >
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <div class="p-4 text-center text-sm opacity-50">
        {albumId
          ? "No songs found in this album."
          : "Select an album to view songs."}
      </div>
    {/if}
  </div>
</div>
