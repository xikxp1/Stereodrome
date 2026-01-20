<script lang="ts">
  import { connection } from "$lib/stores/connection.svelte";
  import { playlistStore, type Playlist } from "$lib/stores/playlist.svelte";
  import { syncLibrary } from "$lib/api/commands";
  import { queryClient } from "$lib/db/queryClient";
  import {
    Music,
    User,
    Disc,
    List,
    Globe,
    Plus,
    ListMusic,
    RefreshCw,
    LogOut,
  } from "lucide-svelte";

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

<div class="sidebar flex flex-col h-full select-none overflow-hidden">
  <!-- Library Section -->
  <div class="flex-1 overflow-y-auto overflow-x-hidden py-1">
    <div class="sidebar-section-title">Library</div>

    <button
      class="sidebar-item w-full"
      class:active={activeView === "music"}
      onclick={() => selectView("music")}
    >
      <Music class="sidebar-item-icon" />
      <span>Music</span>
    </button>

    <button
      class="sidebar-item w-full"
      class:active={activeView === "artists"}
      onclick={() => selectView("artists")}
    >
      <User class="sidebar-item-icon" />
      <span>Artists</span>
    </button>

    <button
      class="sidebar-item w-full"
      class:active={activeView === "albums"}
      onclick={() => selectView("albums")}
    >
      <Disc class="sidebar-item-icon" />
      <span>Albums</span>
    </button>

    <button
      class="sidebar-item w-full"
      class:active={activeView === "songs"}
      onclick={() => selectView("songs")}
    >
      <List class="sidebar-item-icon" />
      <span>Songs</span>
    </button>

    <button
      class="sidebar-item w-full"
      class:active={activeView === "genres"}
      onclick={() => selectView("genres")}
    >
      <Globe class="sidebar-item-icon" />
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
        <Plus class="sidebar-item-icon" />
        <span>New Playlist</span>
      </button>
    {/if}

    {#each playlistStore.playlists as playlist (playlist.id)}
      <button
        class="sidebar-item w-full"
        class:active={selectedPlaylistId === playlist.id}
        onclick={() => selectPlaylist(playlist)}
      >
        <ListMusic class="sidebar-item-icon" />
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
        <p class="text-[10px] text-base-content/50 truncate text-center">
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
        <span class="loading loading-spinner loading-xs"></span>
        <span class="ml-1">Syncing...</span>
      {:else}
        <RefreshCw class="sidebar-item-icon" />
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
      <LogOut class="sidebar-item-icon" />
      <span>Disconnect</span>
    </button>
  </div>
</div>
