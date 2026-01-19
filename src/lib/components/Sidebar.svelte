<script lang="ts">
  import { connection } from "$lib/stores/connection.svelte";
  import { playlistStore, type Playlist } from "$lib/stores/playlist.svelte";
  import { syncLibrary } from "$lib/api/commands";
  import { queryClient } from "$lib/db/queryClient";

  interface Props {
    activeView?: string;
    onViewChange?: (view: string) => void;
    onPlaylistSelect?: (playlist: Playlist | null) => void;
    selectedPlaylistId?: string | null;
    onSync?: () => void;
  }

  let {
    activeView = "music",
    onViewChange,
    onPlaylistSelect,
    selectedPlaylistId = null,
    onSync,
  }: Props = $props();

  let isSyncing = $state(false);
  let syncError = $state<string | null>(null);
  let showCreatePlaylist = $state(false);
  let newPlaylistName = $state("");

  // Load playlists when component mounts
  $effect(() => {
    if (connection.status.connected) {
      playlistStore.loadPlaylists();
    }
  });

  async function handleSync() {
    isSyncing = true;
    syncError = null;
    try {
      await syncLibrary();
      await queryClient.invalidateQueries({ queryKey: ["artists"] });
      await queryClient.invalidateQueries({ queryKey: ["albums"] });
      await queryClient.invalidateQueries({ queryKey: ["songs"] });
      await playlistStore.loadPlaylists();
      onSync?.();
    } catch (e) {
      syncError = e instanceof Error ? e.message : String(e);
    } finally {
      isSyncing = false;
    }
  }

  function selectView(view: string) {
    onViewChange?.(view);
    onPlaylistSelect?.(null); // Deselect playlist when changing views
  }

  function selectPlaylist(playlist: Playlist) {
    onPlaylistSelect?.(playlist);
  }

  async function handleCreatePlaylist() {
    if (!newPlaylistName.trim()) return;
    const playlist = await playlistStore.createPlaylist(newPlaylistName.trim());
    if (playlist) {
      newPlaylistName = "";
      showCreatePlaylist = false;
      selectPlaylist(playlist);
    }
  }
</script>

<div class="sidebar flex flex-col h-full select-none">
  <!-- Library Section -->
  <div class="flex-1 overflow-y-auto py-1">
    <div class="sidebar-section-title">Library</div>

    <button
      class="sidebar-item w-full"
      class:active={activeView === "music"}
      onclick={() => selectView("music")}
    >
      <svg class="sidebar-item-icon" viewBox="0 0 24 24" fill="currentColor">
        <path
          d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
        />
      </svg>
      <span>Music</span>
    </button>

    <button
      class="sidebar-item w-full"
      class:active={activeView === "artists"}
      onclick={() => selectView("artists")}
    >
      <svg class="sidebar-item-icon" viewBox="0 0 24 24" fill="currentColor">
        <path
          d="M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z"
        />
      </svg>
      <span>Artists</span>
    </button>

    <button
      class="sidebar-item w-full"
      class:active={activeView === "albums"}
      onclick={() => selectView("albums")}
    >
      <svg class="sidebar-item-icon" viewBox="0 0 24 24" fill="currentColor">
        <path
          d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z"
        />
      </svg>
      <span>Albums</span>
    </button>

    <button
      class="sidebar-item w-full"
      class:active={activeView === "songs"}
      onclick={() => selectView("songs")}
    >
      <svg class="sidebar-item-icon" viewBox="0 0 24 24" fill="currentColor">
        <path d="M3 4h18v2H3V4zm0 7h12v2H3v-2zm0 7h18v2H3v-2z" />
      </svg>
      <span>Songs</span>
    </button>

    <button
      class="sidebar-item w-full"
      class:active={activeView === "genres"}
      onclick={() => selectView("genres")}
    >
      <svg class="sidebar-item-icon" viewBox="0 0 24 24" fill="currentColor">
        <path
          d="M11.99 2C6.47 2 2 6.48 2 12s4.47 10 9.99 10C17.52 22 22 17.52 22 12S17.52 2 11.99 2zm6.93 6h-2.95a15.65 15.65 0 00-1.38-3.56A8.03 8.03 0 0118.92 8zM12 4.04c.83 1.2 1.48 2.53 1.91 3.96h-3.82c.43-1.43 1.08-2.76 1.91-3.96zM4.26 14C4.1 13.36 4 12.69 4 12s.1-1.36.26-2h3.38c-.08.66-.14 1.32-.14 2s.06 1.34.14 2H4.26zm.82 2h2.95c.32 1.25.78 2.45 1.38 3.56A7.987 7.987 0 015.08 16zm2.95-8H5.08a7.987 7.987 0 014.33-3.56A15.65 15.65 0 008.03 8zM12 19.96c-.83-1.2-1.48-2.53-1.91-3.96h3.82c-.43 1.43-1.08 2.76-1.91 3.96zM14.34 14H9.66c-.09-.66-.16-1.32-.16-2s.07-1.35.16-2h4.68c.09.65.16 1.32.16 2s-.07 1.34-.16 2zm.25 5.56c.6-1.11 1.06-2.31 1.38-3.56h2.95a8.03 8.03 0 01-4.33 3.56zM16.36 14c.08-.66.14-1.32.14-2s-.06-1.34-.14-2h3.38c.16.64.26 1.31.26 2s-.1 1.36-.26 2h-3.38z"
        />
      </svg>
      <span>Genres</span>
    </button>

    <!-- Playlists Section -->
    <div class="sidebar-section-title mt-4">Playlists</div>

    {#if showCreatePlaylist}
      <div class="px-2 py-1">
        <input
          type="text"
          class="w-full px-2 py-1 text-xs bg-base-200 border border-base-300 rounded focus:outline-none focus:border-primary"
          placeholder="Playlist name..."
          bind:value={newPlaylistName}
          onkeydown={(e) => {
            if (e.key === "Enter") handleCreatePlaylist();
            if (e.key === "Escape") {
              showCreatePlaylist = false;
              newPlaylistName = "";
            }
          }}
        />
        <div class="flex gap-1 mt-1">
          <button
            class="flex-1 px-2 py-0.5 text-[10px] bg-primary text-primary-content rounded hover:bg-primary/90"
            onclick={handleCreatePlaylist}
          >
            Create
          </button>
          <button
            class="flex-1 px-2 py-0.5 text-[10px] bg-base-300 rounded hover:bg-base-300/80"
            onclick={() => {
              showCreatePlaylist = false;
              newPlaylistName = "";
            }}
          >
            Cancel
          </button>
        </div>
      </div>
    {:else}
      <button
        class="sidebar-item w-full"
        onclick={() => (showCreatePlaylist = true)}
      >
        <svg class="sidebar-item-icon" viewBox="0 0 24 24" fill="currentColor">
          <path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z" />
        </svg>
        <span>New Playlist</span>
      </button>
    {/if}

    {#each playlistStore.playlists as playlist (playlist.id)}
      <button
        class="sidebar-item w-full"
        class:active={selectedPlaylistId === playlist.id}
        onclick={() => selectPlaylist(playlist)}
      >
        <svg class="sidebar-item-icon" viewBox="0 0 24 24" fill="currentColor">
          <path
            d="M15 6H3v2h12V6zm0 4H3v2h12v-2zM3 16h8v-2H3v2zM17 6v8.18c-.31-.11-.65-.18-1-.18-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3V8h3V6h-5z"
          />
        </svg>
        <span class="truncate">{playlist.name}</span>
        <span class="text-[10px] text-base-content/40 ml-auto"
          >{playlist.song_count}</span
        >
      </button>
    {/each}
  </div>

  <!-- Bottom Actions -->
  <div class="p-3 border-t border-base-300/50 space-y-2">
    {#if connection.status.connected}
      <div class="px-2 mb-2">
        <p class="text-[10px] text-base-content/50 truncate">
          {connection.status.server_url}
        </p>
      </div>
    {/if}

    <button
      class="sidebar-item w-full justify-center !mx-0 !rounded bg-base-300/50 hover:!bg-base-300"
      onclick={handleSync}
      disabled={isSyncing || !connection.status.connected}
    >
      {#if isSyncing}
        <div class="loading-dots">
          <span></span>
          <span></span>
          <span></span>
        </div>
        <span class="ml-1">Syncing...</span>
      {:else}
        <svg class="sidebar-item-icon" viewBox="0 0 24 24" fill="currentColor">
          <path
            d="M12 4V1L8 5l4 4V6c3.31 0 6 2.69 6 6 0 1.01-.25 1.97-.7 2.8l1.46 1.46A7.93 7.93 0 0020 12c0-4.42-3.58-8-8-8zm0 14c-3.31 0-6-2.69-6-6 0-1.01.25-1.97.7-2.8L5.24 7.74A7.93 7.93 0 004 12c0 4.42 3.58 8 8 8v3l4-4-4-4v3z"
          />
        </svg>
        <span>Sync Library</span>
      {/if}
    </button>

    {#if syncError}
      <p class="text-[10px] text-error px-2">{syncError}</p>
    {/if}

    <button
      class="sidebar-item w-full justify-center !mx-0 !rounded text-error/80 hover:!bg-error/10"
      onclick={() => connection.disconnect()}
    >
      <svg class="sidebar-item-icon" viewBox="0 0 24 24" fill="currentColor">
        <path
          d="M17 7l-1.41 1.41L18.17 11H8v2h10.17l-2.58 2.58L17 17l5-5-5-5zM4 5h8V3H4c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h8v-2H4V5z"
        />
      </svg>
      <span>Disconnect</span>
    </button>
  </div>
</div>
