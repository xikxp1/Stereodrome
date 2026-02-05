<script lang="ts">
  import { connection } from "$lib/stores/connection.svelte";
  import { playlistStore } from "$lib/stores/playlist.svelte";
  import { syncLibrary } from "$lib/api/commands";
  import { queryClient } from "$lib/db/queryClient";
  import type { Playlist } from "$lib/types";
  import {
    Music,
    User,
    Disc,
    Plus,
    ListMusic,
    RefreshCw,
    LogOut,
    Pencil,
    Trash2,
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

  // Context menu state
  let contextMenu = $state<{
    x: number;
    y: number;
    playlist: Playlist;
  } | null>(null);

  // Rename state
  let renamingPlaylistId = $state<string | null>(null);
  let renameValue = $state("");

  // Load playlists when component mounts
  $effect(() => {
    if (connection.status.connected) {
      playlistStore.loadPlaylists();
    }
  });

  // Close context menu on click outside
  $effect(() => {
    if (contextMenu) {
      const handler = () => {
        contextMenu = null;
      };
      window.addEventListener("click", handler);
      return () => window.removeEventListener("click", handler);
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
      await playlistStore.syncPlaylists();
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

  function handlePlaylistContextMenu(e: MouseEvent, playlist: Playlist) {
    e.preventDefault();
    contextMenu = { x: e.clientX, y: e.clientY, playlist };
  }

  function startRename(playlist: Playlist) {
    contextMenu = null;
    renamingPlaylistId = playlist.id;
    renameValue = playlist.name;
  }

  async function handleRename() {
    if (!renameValue.trim() || !renamingPlaylistId) return;
    await playlistStore.updatePlaylist(renamingPlaylistId, renameValue.trim());
    renamingPlaylistId = null;
    renameValue = "";
  }

  function cancelRename() {
    renamingPlaylistId = null;
    renameValue = "";
  }

  async function handleDelete(playlist: Playlist) {
    contextMenu = null;
    await playlistStore.deletePlaylist(playlist.id);
    if (selectedPlaylistId === playlist.id) {
      onPlaylistSelect?.(null);
    }
  }
</script>

<div
  class="flex flex-col h-full select-none overflow-hidden bg-base-200 border-r border-base-300"
>
  <div class="flex-1 overflow-y-auto overflow-x-hidden">
    <div class="py-2">
      <div class="sidebar-title">Library</div>
      <button
        class="sidebar-item"
        class:active={activeView === "music" && !selectedPlaylistId}
        onclick={() => selectView("music")}
      >
        <Music class="size-4" />
        Music
      </button>
      <button
        class="sidebar-item"
        class:active={activeView === "artists" && !selectedPlaylistId}
        onclick={() => selectView("artists")}
      >
        <User class="size-4" />
        Artists
      </button>
      <button
        class="sidebar-item"
        class:active={activeView === "albums" && !selectedPlaylistId}
        onclick={() => selectView("albums")}
      >
        <Disc class="size-4" />
        Albums
      </button>

      <div class="sidebar-title mt-4">Playlists</div>

      {#if showCreatePlaylist}
        <div class="px-2 py-1">
          <input
            type="text"
            class="w-full px-2 py-1 text-xs bg-base-100 border border-base-300 rounded focus:outline-none focus:border-primary"
            placeholder="Playlist name..."
            autocomplete="off"
            autocorrect="off"
            autocapitalize="off"
            spellcheck="false"
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
              class="flex-1 px-2 py-0.5 text-[11px] bg-primary hover:bg-primary/50 text-primary-content rounded"
              onclick={handleCreatePlaylist}
            >
              Create
            </button>
            <button
              class="flex-1 px-2 py-0.5 text-[11px] rounded bg-base-300 hover:bg-base-300/50"
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
          class="sidebar-item"
          onclick={() => (showCreatePlaylist = true)}
        >
          <Plus class="size-4" />
          New Playlist
        </button>
      {/if}

      {#each playlistStore.playlists as playlist (playlist.id)}
        {#if renamingPlaylistId === playlist.id}
          <div class="px-2 py-1">
            <input
              type="text"
              class="w-full px-2 py-1 text-xs bg-base-100 border border-base-300 rounded focus:outline-none focus:border-primary"
              autocomplete="off"
              autocorrect="off"
              autocapitalize="off"
              spellcheck="false"
              bind:value={renameValue}
              onkeydown={(e) => {
                if (e.key === "Enter") handleRename();
                if (e.key === "Escape") cancelRename();
              }}
            />
            <div class="flex gap-1 mt-1">
              <button
                class="flex-1 px-2 py-0.5 text-[11px] bg-primary hover:bg-primary/50 text-primary-content rounded"
                onclick={handleRename}
              >
                Save
              </button>
              <button
                class="flex-1 px-2 py-0.5 text-[11px] rounded bg-base-300 hover:bg-base-300/50"
                onclick={cancelRename}
              >
                Cancel
              </button>
            </div>
          </div>
        {:else}
          <button
            class="sidebar-item"
            class:active={selectedPlaylistId === playlist.id}
            onclick={() => selectPlaylist(playlist)}
            oncontextmenu={(e) => handlePlaylistContextMenu(e, playlist)}
          >
            <ListMusic class="size-4" />
            <span class="truncate flex-1">{playlist.name}</span>
            <span class="text-xs opacity-50">{playlist.song_count}</span>
          </button>
        {/if}
      {/each}
    </div>
  </div>

  <div class="p-2 border-t border-base-300 space-y-1">
    {#if connection.status.connected}
      <p class="text-[10px] opacity-50 truncate text-center mb-1">
        {connection.status.server_url}
      </p>
    {/if}

    <button
      class="flex items-center justify-center gap-1.5 w-full px-2 py-1 text-xs rounded hover:bg-base-300 disabled:opacity-50"
      onclick={handleSync}
      disabled={isSyncing || !connection.status.connected}
    >
      {#if isSyncing}
        <span class="loading loading-spinner loading-xs"></span>
        Syncing...
      {:else}
        <RefreshCw class="size-3" />
        Sync Library
      {/if}
    </button>

    {#if syncError}
      <p class="text-[10px] text-error px-2">{syncError}</p>
    {/if}

    <button
      class="flex items-center justify-center gap-1.5 w-full px-2 py-1 text-xs text-error/70 rounded hover:bg-error/10"
      onclick={() => connection.disconnect()}
    >
      <LogOut class="size-3" />
      Disconnect
    </button>
  </div>
</div>

<!-- Context menu -->
{#if contextMenu}
  <div
    class="bg-base-100 border border-base-300 rounded-lg shadow-lg fixed z-1000 min-w-35 py-1"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
  >
    <button
      class="context-menu-item"
      onclick={() => startRename(contextMenu!.playlist)}
    >
      <Pencil class="size-3" />
      Rename
    </button>
    <button
      class="context-menu-item text-error"
      onclick={() => handleDelete(contextMenu!.playlist)}
    >
      <Trash2 class="size-3" />
      Delete
    </button>
  </div>
{/if}

<style>
  .sidebar-title {
    padding: 0.25rem 0.75rem;
    font-size: 0.6875rem;
    font-weight: 600;
    text-transform: uppercase;
    color: oklch(50% 0.01 250);
    letter-spacing: 0.025em;
  }

  .sidebar-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.375rem 0.75rem;
    font-size: 0.8125rem;
    text-align: left;
    color: oklch(30% 0.01 250);
    border: none;
    background: transparent;
    cursor: pointer;
  }

  .sidebar-item:hover {
    background: oklch(90% 0.08 250);
  }

  .sidebar-item.active {
    background: linear-gradient(
      to bottom,
      oklch(58% 0.2 250),
      oklch(52% 0.22 250)
    );
    color: white;
  }

  .sidebar-item.active:hover {
    background: linear-gradient(
      to bottom,
      oklch(58% 0.2 250),
      oklch(52% 0.22 250)
    );
  }

  .context-menu-item {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    width: 100%;
    padding: 0.25rem 0.5rem;
    font-size: 0.75rem;
    text-align: left;
    color: oklch(30% 0.01 250);
    border: none;
    background: transparent;
    cursor: pointer;
  }

  .context-menu-item:hover {
    background: oklch(90% 0.08 250);
  }
</style>
