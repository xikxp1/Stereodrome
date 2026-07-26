import { invoke } from "@tauri-apps/api/core";

import { dispatch } from "$lib/api/core";
import {
  reconcileSavedPlaylistsOffline,
  removeSongsFromPlaylist as removeSongsFromPlaylistCommand,
  setPlaylistSavedOffline,
} from "$lib/api/commands";
import { logError } from "$lib/services/logging";
import type { Playlist, Song } from "$lib/types";

export interface PlaylistSong extends Song {
  position: number;
}

class PlaylistStore {
  playlists = $state<Playlist[]>([]);
  currentPlaylist = $state<Playlist | null>(null);
  currentPlaylistSongs = $state<PlaylistSong[]>([]);
  isLoading = $state(false);
  private readonly membershipMutationPlaylistIds = new Set<string>();

  private async mutatePlaylistMembership(
    playlistId: string,
    mutation: () => Promise<unknown>,
    failureMessage: string
  ) {
    if (this.membershipMutationPlaylistIds.has(playlistId)) {
      return;
    }

    this.membershipMutationPlaylistIds.add(playlistId);
    try {
      await mutation();
      await this.loadPlaylists();

      // Keep the lock until affected song positions have been refreshed.
      if (this.currentPlaylist?.id === playlistId) {
        await this.loadPlaylistSongs(playlistId);
      }
    } catch (cause) {
      logError(failureMessage, cause);
    } finally {
      this.membershipMutationPlaylistIds.delete(playlistId);
    }
  }

  async syncPlaylists() {
    try {
      await invoke("sync_playlists");
      await this.loadPlaylists();
    } catch (cause) {
      logError("Failed to sync playlists", cause);
    }
  }

  async loadPlaylists() {
    this.isLoading = true;
    try {
      this.playlists = await dispatch({ type: "get-playlists" });
    } catch (cause) {
      logError("Failed to load playlists", cause);
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
    } catch (cause) {
      logError("Failed to load playlist songs", cause);
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
      const playlist = await dispatch({
        type: "create-playlist",
        name,
        song_ids: songIds ?? [],
      });
      await this.loadPlaylists();
      return playlist;
    } catch (cause) {
      logError("Failed to create playlist", cause);
      return null;
    }
  }

  async updatePlaylist(playlistId: string, name: string) {
    try {
      await dispatch({
        type: "rename-playlist",
        playlist_id: playlistId,
        name,
      });
      await this.loadPlaylists();

      // Update current playlist if it's the one being edited
      if (this.currentPlaylist?.id === playlistId) {
        this.currentPlaylist = {
          ...this.currentPlaylist,
          name,
        };
      }
    } catch (cause) {
      logError("Failed to update playlist", cause);
    }
  }

  async deletePlaylist(playlistId: string) {
    try {
      await dispatch({ type: "delete-playlist", playlist_id: playlistId });
      await this.loadPlaylists();

      // Clear current playlist if it was deleted
      if (this.currentPlaylist?.id === playlistId) {
        this.currentPlaylist = null;
        this.currentPlaylistSongs = [];
      }
    } catch (cause) {
      logError("Failed to delete playlist", cause);
    }
  }

  async addSongsToPlaylist(playlistId: string, songIds: string[]) {
    await this.mutatePlaylistMembership(
      playlistId,
      () =>
        dispatch({
          type: "add-songs-to-playlist",
          playlist_id: playlistId,
          song_ids: songIds,
        }),
      "Failed to add songs to playlist"
    );
  }

  async removeSongFromPlaylist(playlistId: string, position: number) {
    await this.removeSongsFromPlaylist(playlistId, [position]);
  }

  async removeSongsFromPlaylist(playlistId: string, positions: number[]) {
    const uniquePositions = [...new Set(positions)].sort((a, b) => a - b);
    if (uniquePositions.length === 0) {
      return;
    }

    await this.mutatePlaylistMembership(
      playlistId,
      () => removeSongsFromPlaylistCommand(playlistId, uniquePositions),
      "Failed to remove song from playlist"
    );
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
    } catch (cause) {
      logError("Failed to update saved playlist offline state", cause);
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
    } catch (cause) {
      logError(
        "Failed to reconcile saved playlists for offline listening",
        cause
      );
    }
  }
}

export const playlistStore = new PlaylistStore();
