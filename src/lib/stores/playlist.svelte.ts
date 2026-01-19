import { invoke } from "@tauri-apps/api/core";
import type { Song } from "$lib/types";

export interface Playlist {
  id: string;
  name: string;
  song_count: number;
  duration: number;
  created_at: string;
  changed_at: string;
}

export interface PlaylistSong extends Song {
  position: number;
}

class PlaylistStore {
  playlists = $state<Playlist[]>([]);
  currentPlaylist = $state<Playlist | null>(null);
  currentPlaylistSongs = $state<PlaylistSong[]>([]);
  isLoading = $state(false);

  async loadPlaylists() {
    this.isLoading = true;
    try {
      this.playlists = await invoke<Playlist[]>("get_playlists");
    } catch (e) {
      console.error("Failed to load playlists:", e);
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
      console.error("Failed to load playlist songs:", e);
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
      console.error("Failed to create playlist:", e);
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
      console.error("Failed to update playlist:", e);
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
      console.error("Failed to delete playlist:", e);
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
      console.error("Failed to add songs to playlist:", e);
    }
  }

  async removeSongFromPlaylist(playlistId: string, songId: string) {
    try {
      await invoke("remove_song_from_playlist", { playlistId, songId });
      await this.loadPlaylists();

      // Refresh current playlist songs if affected
      if (this.currentPlaylist?.id === playlistId) {
        await this.loadPlaylistSongs(playlistId);
      }
    } catch (e) {
      console.error("Failed to remove song from playlist:", e);
    }
  }

  async reorderPlaylist(playlistId: string, songIds: string[]) {
    try {
      await invoke("reorder_playlist", { playlistId, songIds });

      // Refresh current playlist songs if affected
      if (this.currentPlaylist?.id === playlistId) {
        await this.loadPlaylistSongs(playlistId);
      }
    } catch (e) {
      console.error("Failed to reorder playlist:", e);
    }
  }
}

export const playlistStore = new PlaylistStore();
