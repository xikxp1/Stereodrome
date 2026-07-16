import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { error } from "@tauri-apps/plugin-log";
import type { Song } from "$lib/types";
import { queue } from "./queue.svelte";
import { notifications } from "$lib/services/notifications.svelte";
import { setPersistedVolume } from "$lib/api/commands";

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
  private static readonly PERSIST_DEBOUNCE_MS = 250;

  // State
  isPlaying = $state(false);
  position = $state(0);
  duration = $state(0);
  volume = $state(0.8);

  // Current track info from backend (for TransportBar display)
  currentTrack = $state<CurrentTrack | null>(null);

  // Event listeners
  private unlistenState: UnlistenFn | null = null;
  private unlistenEnded: UnlistenFn | null = null;
  private persistVolumeTimeout: ReturnType<typeof setTimeout> | null = null;
  private readonly shouldHandleSideEffects =
    getCurrentWindow().label === "main";

  constructor() {
    this.setupEventListeners();
  }

  private toCurrentTrack(song: SongMetadata): CurrentTrack {
    return {
      id: song.id,
      title: song.title,
      artist: song.artist || "Unknown Artist",
      album: song.album || "",
      coverArtId: song.cover_art_id || null,
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
        this.volume = state.volume;

        if (state.song) {
          const nextTrack = this.toCurrentTrack(state.song);

          const songChanged = state.song.id !== this.currentTrack?.id;

          // Notify song change when app is not focused
          if (songChanged && this.shouldHandleSideEffects) {
            notifications.notifySongChange(
              state.song.title,
              state.song.artist,
              state.song.cover_art_id
            );
          }

          if (!this.sameTrackMetadata(this.currentTrack, nextTrack)) {
            this.currentTrack = nextTrack;
          }
        } else {
          if (this.currentTrack !== null) {
            this.currentTrack = null;
          }
        }
      }
    );

    // Listen for playback ended
    this.unlistenEnded = await listen("playback-ended", () => {
      this.isPlaying = false;
      this.position = 0;
      this.duration = 0;
      this.currentTrack = null;
    });

    // Sync startup UI state with backend-applied runtime values (e.g. restored volume).
    void this.refreshStatus();
  }

  async playSong(song: Song) {
    try {
      await invoke("play_song", { songId: song.id });
      this.isPlaying = true;
      this.position = 0;
      this.duration = song.duration || 0;
    } catch (e) {
      error(`Failed to play song: ${e}`);
      throw e;
    }
  }

  async pause() {
    try {
      await invoke("pause_playback");
      this.isPlaying = false;
    } catch (e) {
      error(`Failed to pause: ${e}`);
    }
  }

  async resume() {
    try {
      await invoke("resume_playback");
      this.isPlaying = true;
    } catch (e) {
      error(`Failed to resume: ${e}`);
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
      await invoke("stop_playback");
      this.isPlaying = false;
      this.position = 0;
      this.currentTrack = null;
    } catch (e) {
      error(`Failed to stop: ${e}`);
    }
  }

  async setVolume(volume: number) {
    const clamped = Math.max(0, Math.min(1, volume));
    try {
      await invoke("set_volume", { volume: clamped });
      this.volume = clamped;
      this.scheduleVolumePersistence(clamped);
    } catch (e) {
      error(`Failed to set volume: ${e}`);
    }
  }

  private scheduleVolumePersistence(volume: number) {
    if (!this.shouldHandleSideEffects) return;

    if (this.persistVolumeTimeout) {
      clearTimeout(this.persistVolumeTimeout);
    }

    this.persistVolumeTimeout = setTimeout(() => {
      void setPersistedVolume(volume).catch((e) => {
        error(`Failed to persist volume: ${e}`);
      });
      this.persistVolumeTimeout = null;
    }, PlaybackStore.PERSIST_DEBOUNCE_MS);
  }

  async refreshStatus() {
    try {
      const status = await invoke<PlaybackStatus>("get_playback_status");
      this.isPlaying = status.is_playing;
      this.position = status.position;
      this.duration = status.duration;
      this.volume = status.volume;
    } catch (e) {
      error(`Failed to get playback status: ${e}`);
    }
  }

  // Cleanup
  destroy() {
    if (this.unlistenState) {
      this.unlistenState();
    }
    if (this.unlistenEnded) {
      this.unlistenEnded();
    }
    if (this.persistVolumeTimeout) {
      clearTimeout(this.persistVolumeTimeout);
      this.persistVolumeTimeout = null;
    }
  }
}

export const playback = new PlaybackStore();
