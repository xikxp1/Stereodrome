import { invoke } from "@tauri-apps/api/core";
import { error } from "@tauri-apps/plugin-log";
import {
  reconcileSavedPlaylistsOffline,
  removeSongsFromPlaylist as removeSongsFromPlaylistCommand,
  setPlaylistSavedOffline,
} from "$lib/api/commands";
import type { Playlist, Song } from "$lib/types";

export interface PlaylistSong extends Song {
  position: number;
}

class PlaylistStore {
  playlists = $state<Playlist[]>([]);
  currentPlaylist = $state<Playlist | null>(null);
  currentPlaylistSongs = $state<PlaylistSong[]>([]);
  isLoading = $state(false);

  async syncPlaylists() {
    try {
      await invoke("sync_playlists");
      await this.loadPlaylists();
    } catch (e) {
      error(`Failed to sync playlists: ${e}`);
    }
  }

  async loadPlaylists() {
    this.isLoading = true;
    try {
      this.playlists = await invoke<Playlist[]>("get_playlists");
    } catch (e) {
      error(`Failed to load playlists: ${e}`);
    } finally {
      this.isLoading = false;
    }
  }

  async loadPlaylistSongs(playlistId: string) {
    this.isLoading = true;
    try {
      this.currentPlaylistSongs = await invoke<PlaylistSong[]>(
        "get_playlist_songs",
        { playlistId }
      );
    } catch (e) {
      error(`Failed to load playlist songs: ${e}`);
    } finally {
      this.isLoading = false;
    }
  }

  async selectPlaylist(playlist: Playlist | null) {
    this.currentPlaylist = playlist;
    if (playlist) {
      await this.loadPlaylistSongs(playlist.id);
    } else {
      this.currentPlaylistSongs = [];
    }
  }

  async createPlaylist(
    name: string,
    songIds?: string[]
  ): Promise<Playlist | null> {
    try {
      const playlist = await invoke<Playlist>("create_playlist", {
        name,
        songIds,
      });
      await this.loadPlaylists();
      return playlist;
    } catch (e) {
      error(`Failed to create playlist: ${e}`);
      return null;
    }
  }

  async updatePlaylist(playlistId: string, name: string) {
    try {
      await invoke("update_playlist", { playlistId, name });
      await this.loadPlaylists();

      // Update current playlist if it's the one being edited
      if (this.currentPlaylist?.id === playlistId) {
        this.currentPlaylist = {
          ...this.currentPlaylist,
          name,
        };
      }
    } catch (e) {
      error(`Failed to update playlist: ${e}`);
    }
  }

  async deletePlaylist(playlistId: string) {
    try {
      await invoke("delete_playlist", { playlistId });
      await this.loadPlaylists();

      // Clear current playlist if it was deleted
      if (this.currentPlaylist?.id === playlistId) {
        this.currentPlaylist = null;
        this.currentPlaylistSongs = [];
      }
    } catch (e) {
      error(`Failed to delete playlist: ${e}`);
    }
  }

  async addSongsToPlaylist(playlistId: string, songIds: string[]) {
    try {
      await invoke("add_songs_to_playlist", { playlistId, songIds });
      await this.loadPlaylists();

      // Refresh current playlist songs if affected
      if (this.currentPlaylist?.id === playlistId) {
        await this.loadPlaylistSongs(playlistId);
      }
    } catch (e) {
      error(`Failed to add songs to playlist: ${e}`);
    }
  }

  async removeSongFromPlaylist(playlistId: string, position: number) {
    await this.removeSongsFromPlaylist(playlistId, [position]);
  }

  async removeSongsFromPlaylist(playlistId: string, positions: number[]) {
    const uniquePositions = [...new Set(positions)].sort((a, b) => a - b);
    if (uniquePositions.length === 0) {
      return;
    }

    try {
      await removeSongsFromPlaylistCommand(playlistId, uniquePositions);
      await this.loadPlaylists();

      // Refresh current playlist songs if affected
      if (this.currentPlaylist?.id === playlistId) {
        await this.loadPlaylistSongs(playlistId);
      }
    } catch (e) {
      error(`Failed to remove song from playlist: ${e}`);
    }
  }

  async setPlaylistSavedOffline(
    playlistId: string,
    savedOffline: boolean
  ): Promise<boolean> {
    try {
      await setPlaylistSavedOffline(playlistId, savedOffline);
      await this.loadPlaylists();

      if (this.currentPlaylist?.id === playlistId) {
        this.currentPlaylist =
          this.playlists.find((p) => p.id === playlistId) ??
          this.currentPlaylist;
        await this.loadPlaylistSongs(playlistId);
      }
      return true;
    } catch (e) {
      error(`Failed to update saved playlist offline state: ${e}`);
      return false;
    }
  }

  async reconcileSavedPlaylistsOffline() {
    try {
      await reconcileSavedPlaylistsOffline();
      await this.loadPlaylists();
      if (this.currentPlaylist) {
        await this.loadPlaylistSongs(this.currentPlaylist.id);
      }
    } catch (e) {
      error(`Failed to reconcile saved playlists for offline listening: ${e}`);
    }
  }
}

export const playlistStore = new PlaylistStore();
