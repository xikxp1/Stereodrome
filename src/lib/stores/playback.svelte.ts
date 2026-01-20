import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Song } from "$lib/types";
import { queue } from "./queue.svelte";

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
}

interface PlaybackState {
  is_playing: boolean;
  position: number;
  duration: number;
  song: SongMetadata | null;
}

class PlaybackStore {
  // State
  isPlaying = $state(false);
  position = $state(0);
  duration = $state(0);
  volume = $state(0.8);

  // Current track info from backend (for TransportBar display)
  currentTrack = $state<{
    id: string;
    title: string;
    artist: string;
    album: string;
  } | null>(null);

  // Event listeners
  private unlistenState: UnlistenFn | null = null;
  private unlistenEnded: UnlistenFn | null = null;

  constructor() {
    this.setupEventListeners();
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

        if (state.song) {
          this.currentTrack = {
            id: state.song.id,
            title: state.song.title,
            artist: state.song.artist || "Unknown Artist",
            album: state.song.album || "",
          };
        }
      }
    );

    // Listen for playback ended
    this.unlistenEnded = await listen("playback-ended", () => {
      this.isPlaying = false;
      this.position = 0;
      // Queue will handle auto-advance later
    });
  }

  async playSong(song: Song) {
    try {
      await invoke("play_song", { songId: song.id });
      this.isPlaying = true;
      this.position = 0;
      this.duration = song.duration || 0;
    } catch (e) {
      console.error("Failed to play song:", e);
      throw e;
    }
  }

  async pause() {
    try {
      await invoke("pause_playback");
      this.isPlaying = false;
    } catch (e) {
      console.error("Failed to pause:", e);
    }
  }

  async resume() {
    try {
      await invoke("resume_playback");
      this.isPlaying = true;
    } catch (e) {
      console.error("Failed to resume:", e);
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
      console.error("Failed to stop:", e);
    }
  }

  async setVolume(volume: number) {
    const clamped = Math.max(0, Math.min(1, volume));
    try {
      await invoke("set_volume", { volume: clamped });
      this.volume = clamped;
    } catch (e) {
      console.error("Failed to set volume:", e);
    }
  }

  async refreshStatus() {
    try {
      const status = await invoke<PlaybackStatus>("get_playback_status");
      this.isPlaying = status.is_playing;
      this.position = status.position;
      this.duration = status.duration;
      this.volume = status.volume;
    } catch (e) {
      console.error("Failed to get playback status:", e);
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
  }
}

export const playback = new PlaybackStore();
