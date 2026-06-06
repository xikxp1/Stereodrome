<script lang="ts">
  import { tick } from "svelte";
  import { connection } from "$lib/stores/connection.svelte";
  import { playlistStore } from "$lib/stores/playlist.svelte";
  import type { Playlist } from "$lib/types";
  import {
    Music,
    User,
    Disc,
    Plus,
    ListMusic,
    Sparkles,
    Clock,
    TrendingUp,
    CircleCheck,
  } from "lucide-svelte";
  import { showPlaylistContextMenu } from "$lib/services/contextMenu";

  interface Props {
    activeView?: string;
    onViewChange?: (view: string) => void;
    onPlaylistSelect?: (playlist: Playlist | null) => void;
    selectedPlaylistId?: string | null;
  }

  let {
    activeView = "music",
    onViewChange,
    onPlaylistSelect,
    selectedPlaylistId = null,
  }: Props = $props();

  let showCreatePlaylist = $state(false);
  let newPlaylistName = $state("");
  let createPlaylistInput = $state<HTMLInputElement | null>(null);

  // Rename state
  let renamingPlaylistId = $state<string | null>(null);
  let renameValue = $state("");

  // Load cached playlists for configured sessions, including offline restores.
  $effect(() => {
    if (connection.status.server_url) {
      void playlistStore.loadPlaylists();
    }
  });

  function selectView(view: string) {
    onViewChange?.(view);
    onPlaylistSelect?.(null); // Deselect playlist when changing views
  }

  function selectPlaylist(playlist: Playlist) {
    onPlaylistSelect?.(playlist);
  }

  async function openCreatePlaylist() {
    showCreatePlaylist = true;
    await tick();
    createPlaylistInput?.focus();
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
    showPlaylistContextMenu({
      savedOffline: playlist.saved_offline,
      onToggleSavedOffline: async () => {
        await playlistStore.setPlaylistSavedOffline(
          playlist.id,
          !playlist.saved_offline
        );
      },
      onRename: () => {
        renamingPlaylistId = playlist.id;
        renameValue = playlist.name;
      },
      onDelete: async () => {
        await playlistStore.deletePlaylist(playlist.id);
        if (selectedPlaylistId === playlist.id) {
          onPlaylistSelect?.(null);
        }
      },
    });
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
      <button
        class="sidebar-item sub-item"
        class:active={activeView === "recently_added" && !selectedPlaylistId}
        onclick={() => selectView("recently_added")}
      >
        <Sparkles class="size-4" />
        Recently Added
      </button>
      <button
        class="sidebar-item sub-item"
        class:active={activeView === "recently_played" && !selectedPlaylistId}
        onclick={() => selectView("recently_played")}
      >
        <Clock class="size-4" />
        Recently Played
      </button>
      <button
        class="sidebar-item sub-item"
        class:active={activeView === "most_played" && !selectedPlaylistId}
        onclick={() => selectView("most_played")}
      >
        <TrendingUp class="size-4" />
        Most Played
      </button>

      <div class="sidebar-title mt-4">Playlists</div>

      {#if showCreatePlaylist}
        <div class="px-2 py-1">
          <input
            bind:this={createPlaylistInput}
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
        <button class="sidebar-item" onclick={openCreatePlaylist}>
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
            {#if playlist.saved_offline}
              <CircleCheck class="size-3 text-success" />
            {/if}
            <span class="text-xs opacity-50">{playlist.song_count}</span>
          </button>
        {/if}
      {/each}
    </div>
  </div>
</div>

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

  .sidebar-item.sub-item {
    padding-left: 2rem;
    font-size: 0.75rem;
  }
</style>
