import { invoke } from "@tauri-apps/api/core";

import { dispatch } from "$lib/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Song } from "$lib/types";
import { queue } from "./queue.svelte";
import { logError } from "$lib/services/logging";
import { notifications } from "$lib/services/notifications.svelte";

interface PlaybackStatus {
  is_playing: boolean;
  current_song_id: string | null;
  position: number;
  duration: number;
  volume: number;
}

interface SongMetadata {
  id: string;
  title: string;
  artist: string;
  album: string;
  cover_art_id: string | null;
}

interface PlaybackState {
  is_playing: boolean;
  position: number;
  duration: number;
  volume: number;
  song: SongMetadata | null;
}

interface CurrentTrack {
  id: string;
  title: string;
  artist: string;
  album: string;
  coverArtId: string | null;
}

class PlaybackStore {
  // State
  isPlaying = $state(false);
  position = $state(0);
  duration = $state(0);
  volume = $state(0.8);

  // Current track info from backend (for TransportBar display)
  currentTrack = $state<CurrentTrack | null>(null);

  // Event listeners
  private unlistenState: UnlistenFn | null = null;
  private volumeUpdateQueue: Promise<void> = Promise.resolve();
  private pendingVolumeUpdates = 0;
  private readonly shouldHandleSideEffects =
    getCurrentWindow().label === "main";

  constructor() {
    void this.setupEventListeners();
  }

  private toCurrentTrack(song: SongMetadata): CurrentTrack {
    return {
      id: song.id,
      title: song.title,
      artist: song.artist !== "" ? song.artist : "Unknown Artist",
      album: song.album,
      coverArtId:
        song.cover_art_id !== null && song.cover_art_id !== ""
          ? song.cover_art_id
          : null,
    };
  }

  private sameTrackMetadata(
    current: CurrentTrack | null,
    next: CurrentTrack | null
  ): boolean {
    return (
      current?.id === next?.id &&
      current?.title === next?.title &&
      current?.artist === next?.artist &&
      current?.album === next?.album &&
      current?.coverArtId === next?.coverArtId
    );
  }

  private async setupEventListeners() {
    // Listen for combined playback state updates (10Hz)
    this.unlistenState = await listen<PlaybackState>(
      "playback-state",
      (event) => {
        const state = event.payload;
        this.isPlaying = state.is_playing;
        this.position = state.position;
        this.duration = state.duration;
        if (this.pendingVolumeUpdates === 0) {
          this.volume = state.volume;
        }

        if (state.song) {
          const nextTrack = this.toCurrentTrack(state.song);

          const songChanged = state.song.id !== this.currentTrack?.id;

          // Notify song change when app is not focused
          if (songChanged && this.shouldHandleSideEffects) {
            void notifications
              .notifySongChange(
                state.song.title,
                state.song.artist,
                state.song.cover_art_id
              )
              .catch((cause: unknown) => {
                logError("Failed to send song change notification", cause);
              });
          }

          if (!this.sameTrackMetadata(this.currentTrack, nextTrack)) {
            this.currentTrack = nextTrack;
          }
        } else if (this.currentTrack !== null) {
          this.currentTrack = null;
        }
      }
    );

    // Sync startup UI state with backend-applied runtime values (e.g. restored volume).
    void this.refreshStatus();
  }

  async playSong(song: Song) {
    try {
      await dispatch({
        type: "play-selection",
        song_id: song.id,
        song_ids: [song.id],
      });
      this.isPlaying = true;
      this.position = 0;
      this.duration =
        song.duration !== null &&
        song.duration !== 0 &&
        !Number.isNaN(song.duration)
          ? song.duration
          : 0;
    } catch (cause) {
      logError("Failed to play song", cause);
      throw cause;
    }
  }

  async pause() {
    try {
      await dispatch({ type: "pause-playback" });
      this.isPlaying = false;
    } catch (cause) {
      logError("Failed to pause", cause);
    }
  }

  async resume() {
    try {
      await dispatch({ type: "resume-playback" });
      this.isPlaying = true;
    } catch (cause) {
      logError("Failed to resume", cause);
    }
  }

  async togglePlayPause() {
    if (this.isPlaying) {
      await this.pause();
    } else if (this.currentTrack) {
      // Resume paused playback
      await this.resume();
    } else if (queue.currentIndex !== null && queue.items.length > 0) {
      // Nothing playing but queue has items - play the current queue item
      await queue.playQueueItem(queue.currentIndex);
    } else if (queue.items.length > 0) {
      // Queue has items but no current index - start from beginning
      await queue.playQueueItem(0);
    }
  }

  async stop() {
    try {
      await dispatch({ type: "stop-playback" });
      this.isPlaying = false;
      this.position = 0;
      this.currentTrack = null;
    } catch (cause) {
      logError("Failed to stop", cause);
    }
  }

  async setVolume(volume: number) {
    const clamped = Math.max(0, Math.min(1, volume));
    this.volume = clamped;
    this.pendingVolumeUpdates += 1;

    const update = this.volumeUpdateQueue.then(() =>
      dispatch({ type: "set-playback-volume", volume: clamped })
    );
    this.volumeUpdateQueue = update.catch(() => undefined);

    try {
      await update;
    } catch (cause) {
      logError("Failed to set volume", cause);
    } finally {
      this.pendingVolumeUpdates -= 1;
    }
  }

  async refreshStatus() {
    try {
      const status = await invoke<PlaybackStatus>("get_playback_status");
      this.isPlaying = status.is_playing;
      this.position = status.position;
      this.duration = status.duration;
      if (this.pendingVolumeUpdates === 0) {
        this.volume = status.volume;
      }
    } catch (cause) {
      logError("Failed to get playback status", cause);
    }
  }

  // Cleanup
  destroy() {
    if (this.unlistenState) {
      this.unlistenState();
    }
  }
}

export const playback = new PlaybackStore();
