import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Song } from "$lib/types";

interface PlaybackStatus {
  is_playing: boolean;
  current_song_id: string | null;
  position: number;
  duration: number;
  volume: number;
}

interface PositionEvent {
  position: number;
  song_id: string | null;
}

class PlaybackStore {
  // State
  currentSong = $state<Song | null>(null);
  isPlaying = $state(false);
  position = $state(0);
  duration = $state(0);
  volume = $state(0.8);

  // Event listeners
  private unlistenPosition: UnlistenFn | null = null;
  private unlistenEnded: UnlistenFn | null = null;

  constructor() {
    this.setupEventListeners();
  }

  private async setupEventListeners() {
    // Listen for position updates
    this.unlistenPosition = await listen<PositionEvent>(
      "playback-position",
      (event) => {
        this.position = event.payload.position;
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
      this.currentSong = song;
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
    } else if (this.currentSong) {
      await this.resume();
    }
  }

  async stop() {
    try {
      await invoke("stop_playback");
      this.isPlaying = false;
      this.position = 0;
      this.currentSong = null;
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
    if (this.unlistenPosition) {
      this.unlistenPosition();
    }
    if (this.unlistenEnded) {
      this.unlistenEnded();
    }
  }
}

export const playback = new PlaybackStore();
