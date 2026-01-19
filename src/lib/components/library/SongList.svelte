<script lang="ts">
  import type { Song } from "$lib/types";

  interface Props {
    songs?: Song[];
    isLoading?: boolean;
    error?: Error | null;
    selectedSongId?: string | null;
    playingSongId?: string | null;
    onSelect?: (song: Song) => void;
    onPlay?: (song: Song) => void;
  }

  let {
    songs = [],
    isLoading = false,
    error = null,
    selectedSongId = null,
    playingSongId = null,
    onSelect,
    onPlay,
  }: Props = $props();

  import { SvelteSet } from "svelte/reactivity";

  let checkedSongs = new SvelteSet<string>();

  function formatDuration(seconds: number | null): string {
    if (!seconds) return "--:--";
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  }

  function handleRowClick(song: Song) {
    onSelect?.(song);
  }

  function handleRowDoubleClick(song: Song) {
    onPlay?.(song);
  }

  function toggleCheck(songId: string, e: Event) {
    e.stopPropagation();
    if (checkedSongs.has(songId)) {
      checkedSongs.delete(songId);
    } else {
      checkedSongs.add(songId);
    }
  }

  function toggleAllChecked() {
    if (checkedSongs.size === songs.length) {
      checkedSongs.clear();
    } else {
      songs.forEach((s) => checkedSongs.add(s.id));
    }
  }

  const allChecked = $derived(
    songs.length > 0 && checkedSongs.size === songs.length
  );
  const someChecked = $derived(
    checkedSongs.size > 0 && checkedSongs.size < songs.length
  );
</script>

<div class="flex flex-col h-full bg-white select-none">
  {#if isLoading}
    <div class="flex-1 flex items-center justify-center">
      <div class="loading-dots">
        <span></span>
        <span></span>
        <span></span>
      </div>
    </div>
  {:else if error}
    <div class="empty-state">
      <svg
        class="empty-state-icon"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
      >
        <circle cx="12" cy="12" r="10" />
        <path d="M12 8v4m0 4h.01" />
      </svg>
      <div class="empty-state-title">Failed to load songs</div>
      <div class="empty-state-text">{error.message}</div>
    </div>
  {:else if songs.length === 0}
    <div class="empty-state">
      <svg
        class="empty-state-icon"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
      >
        <path
          d="M9 19V6l12-3v13M9 19c0 1.105-1.343 2-3 2s-3-.895-3-2 1.343-2 3-2 3 .895 3 2zm12-3c0 1.105-1.343 2-3 2s-3-.895-3-2 1.343-2 3-2 3 .895 3 2zM9 10l12-3"
        />
      </svg>
      <div class="empty-state-title">No songs</div>
      <div class="empty-state-text">
        Select an artist or album to view songs
      </div>
    </div>
  {:else}
    <div class="flex-1 overflow-auto">
      <table class="song-table">
        <thead class="song-table-header">
          <tr>
            <th class="w-8 text-center">
              <input
                type="checkbox"
                class="itunes-checkbox"
                checked={allChecked}
                indeterminate={someChecked}
                onchange={toggleAllChecked}
              />
            </th>
            <th class="w-10">#</th>
            <th>Name</th>
            <th class="w-16 text-right">Time</th>
            <th class="w-40">Artist</th>
            <th class="w-40">Album</th>
            <th class="w-14">Year</th>
            <th class="w-24">Genre</th>
          </tr>
        </thead>
        <tbody>
          {#each songs as song, index (song.id)}
            <tr
              class="song-row"
              class:selected={selectedSongId === song.id}
              class:playing={playingSongId === song.id}
              onclick={() => handleRowClick(song)}
              ondblclick={() => handleRowDoubleClick(song)}
            >
              <td class="text-center">
                <input
                  type="checkbox"
                  class="itunes-checkbox"
                  checked={checkedSongs.has(song.id)}
                  onchange={(e) => toggleCheck(song.id, e)}
                />
              </td>
              <td class="dimmed">{song.track_number || index + 1}</td>
              <td class="font-medium">
                {#if playingSongId === song.id}
                  <span class="inline-flex items-center gap-1.5">
                    <svg
                      class="w-3 h-3 animate-pulse"
                      viewBox="0 0 24 24"
                      fill="currentColor"
                    >
                      <path d="M3 9v6h4l5 5V4L7 9H3z" />
                    </svg>
                    {song.title}
                  </span>
                {:else}
                  {song.title}
                {/if}
              </td>
              <td class="text-right dimmed tabular-nums"
                >{formatDuration(song.duration)}</td
              >
              <td class="dimmed">{song.artist || "—"}</td>
              <td class="dimmed">{song.album || "—"}</td>
              <td class="dimmed">{song.year || "—"}</td>
              <td class="dimmed">{song.genre || "—"}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
