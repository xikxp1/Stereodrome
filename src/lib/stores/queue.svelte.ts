import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { playback } from "./playback.svelte";
import type { Song } from "$lib/types";

export type RepeatMode = "Off" | "All" | "One";

export interface QueueItem {
  song_id: string;
  title: string;
  artist: string;
  album: string;
  duration: number;
}

interface QueueState {
  items: QueueItem[];
  current_index: number | null;
  shuffle: boolean;
  repeat_mode: RepeatMode;
}

class QueueStore {
  // State
  items = $state<QueueItem[]>([]);
  currentIndex = $state<number | null>(null);
  shuffle = $state(false);
  repeatMode = $state<RepeatMode>("Off");

  // Event listeners
  private unlistenEnded: UnlistenFn | null = null;
  private unlistenQueueEnded: UnlistenFn | null = null;

  constructor() {
    this.setupEventListeners();
  }

  private async setupEventListeners() {
    // Listen for playback ended to auto-advance
    this.unlistenEnded = await listen("playback-ended", async () => {
      await this.playNext();
    });

    // Listen for queue ended
    this.unlistenQueueEnded = await listen("queue-ended", () => {
      // Queue finished, reset state
      this.currentIndex = null;
    });
  }

  private songToQueueItem(song: Song): QueueItem {
    return {
      song_id: song.id,
      title: song.title,
      artist: song.artist || "Unknown Artist",
      album: song.album || "Unknown Album",
      duration: song.duration || 0,
    };
  }

  async refresh() {
    try {
      const state = await invoke<QueueState>("get_queue");
      this.items = state.items;
      this.currentIndex = state.current_index;
      this.shuffle = state.shuffle;
      this.repeatMode = state.repeat_mode;
    } catch (e) {
      console.error("Failed to refresh queue:", e);
    }
  }

  async addSong(song: Song) {
    const item = this.songToQueueItem(song);
    try {
      await invoke("add_to_queue", { item });
      this.items = [...this.items, item];
    } catch (e) {
      console.error("Failed to add to queue:", e);
    }
  }

  async addSongs(songs: Song[]) {
    const items = songs.map((s) => this.songToQueueItem(s));
    try {
      await invoke("add_songs_to_queue", { items });
      this.items = [...this.items, ...items];
    } catch (e) {
      console.error("Failed to add songs to queue:", e);
    }
  }

  async playNext(song?: Song) {
    if (song) {
      // Insert song as next to play
      const item = this.songToQueueItem(song);
      try {
        await invoke("insert_next_in_queue", { item });
        await this.refresh();
      } catch (e) {
        console.error("Failed to insert next:", e);
      }
    } else {
      // Play the next song in queue
      try {
        const hasNext = await invoke<boolean>("play_next");
        if (hasNext) {
          await this.refresh();
        }
      } catch (e) {
        console.error("Failed to play next:", e);
      }
    }
  }

  async playPrevious() {
    try {
      await invoke<boolean>("play_previous");
      await this.refresh();
    } catch (e) {
      console.error("Failed to play previous:", e);
    }
  }

  async playQueueItem(index: number) {
    try {
      await invoke("play_queue_item", { index });
      this.currentIndex = index;
    } catch (e) {
      console.error("Failed to play queue item:", e);
    }
  }

  async removeFromQueue(index: number) {
    try {
      await invoke("remove_from_queue", { index });
      await this.refresh();
    } catch (e) {
      console.error("Failed to remove from queue:", e);
    }
  }

  async clearQueue() {
    try {
      await invoke("clear_queue");
      this.items = [];
      this.currentIndex = null;
    } catch (e) {
      console.error("Failed to clear queue:", e);
    }
  }

  async moveItem(from: number, to: number) {
    try {
      await invoke("move_queue_item", { from, to });
      await this.refresh();
    } catch (e) {
      console.error("Failed to move queue item:", e);
    }
  }

  async toggleShuffle() {
    try {
      const newValue = await invoke<boolean>("toggle_shuffle");
      this.shuffle = newValue;
      await this.refresh(); // Refresh to get reordered items
    } catch (e) {
      console.error("Failed to toggle shuffle:", e);
    }
  }

  async cycleRepeatMode() {
    try {
      const newMode = await invoke<RepeatMode>("cycle_repeat_mode");
      this.repeatMode = newMode;
    } catch (e) {
      console.error("Failed to cycle repeat mode:", e);
    }
  }

  async setRepeatMode(mode: RepeatMode) {
    try {
      await invoke("set_repeat_mode", { mode });
      this.repeatMode = mode;
    } catch (e) {
      console.error("Failed to set repeat mode:", e);
    }
  }

  // Helper to play a song and optionally set up queue
  async playSongWithQueue(song: Song, allSongs?: Song[]) {
    // If allSongs provided, set up queue with all songs starting from this one
    if (allSongs && allSongs.length > 0) {
      await this.clearQueue();
      await this.addSongs(allSongs);

      // Find the index of the song to play
      const index = allSongs.findIndex((s) => s.id === song.id);
      if (index >= 0) {
        await this.playQueueItem(index);
      }
    } else {
      // Just play the single song
      await playback.playSong(song);
    }
  }

  // Cleanup
  destroy() {
    if (this.unlistenEnded) {
      this.unlistenEnded();
    }
    if (this.unlistenQueueEnded) {
      this.unlistenQueueEnded();
    }
  }
}

export const queue = new QueueStore();
