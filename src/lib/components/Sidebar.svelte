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

<div
  class="flex flex-col h-full select-none overflow-hidden bg-base-200 border-r border-base-300"
>
  <div class="flex-1 overflow-y-auto overflow-x-hidden">
    <ul class="menu menu-sm p-2">
      <li class="menu-title">Library</li>
      <li>
        <button
          class:active={activeView === "music"}
          onclick={() => selectView("music")}
        >
          <Music class="size-4" />
          Music
        </button>
      </li>
      <li>
        <button
          class:active={activeView === "artists"}
          onclick={() => selectView("artists")}
        >
          <User class="size-4" />
          Artists
        </button>
      </li>
      <li>
        <button
          class:active={activeView === "albums"}
          onclick={() => selectView("albums")}
        >
          <Disc class="size-4" />
          Albums
        </button>
      </li>
      <li>
        <button
          class:active={activeView === "songs"}
          onclick={() => selectView("songs")}
        >
          <List class="size-4" />
          Songs
        </button>
      </li>
      <li>
        <button
          class:active={activeView === "genres"}
          onclick={() => selectView("genres")}
        >
          <Globe class="size-4" />
          Genres
        </button>
      </li>

      <li class="menu-title mt-4">Playlists</li>

      {#if showCreatePlaylist}
        <li class="px-2 py-1">
          <div class="flex flex-col gap-1 p-0 hover:bg-transparent">
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
            <div class="flex gap-1">
              <button
                class="flex-1 px-2 py-0.5 text-[11px] bg-primary text-primary-content rounded hover:opacity-90"
                onclick={handleCreatePlaylist}
              >
                Create
              </button>
              <button
                class="flex-1 px-2 py-0.5 text-[11px] rounded hover:bg-base-300"
                onclick={() => {
                  showCreatePlaylist = false;
                  newPlaylistName = "";
                }}
              >
                Cancel
              </button>
            </div>
          </div>
        </li>
      {:else}
        <li>
          <button onclick={() => (showCreatePlaylist = true)}>
            <Plus class="size-4" />
            New Playlist
          </button>
        </li>
      {/if}

      {#each playlistStore.playlists as playlist (playlist.id)}
        <li>
          <button
            class:active={selectedPlaylistId === playlist.id}
            onclick={() => selectPlaylist(playlist)}
          >
            <ListMusic class="size-4" />
            <span class="truncate flex-1">{playlist.name}</span>
            <span class="badge badge-ghost badge-xs">{playlist.song_count}</span
            >
          </button>
        </li>
      {/each}
    </ul>
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
