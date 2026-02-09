import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { error } from "@tauri-apps/plugin-log";
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
  pending_navigation_index: number | null;
}

class QueueStore {
  // State - reflects backend state
  items = $state<QueueItem[]>([]);
  currentIndex = $state<number | null>(null);
  shuffle = $state(false);
  repeatMode = $state<RepeatMode>("Off");
  pendingNavigationIndex = $state<number | null>(null);

  // Event listeners
  private unlistenChanged: UnlistenFn | null = null;
  private unlistenEnded: UnlistenFn | null = null;
  private unlistenQueueEnded: UnlistenFn | null = null;
  private readonly shouldAutoAdvance = getCurrentWindow().label === "main";

  constructor() {
    this.init();
  }

  private async init() {
    // Load initial state from backend
    await this.loadFromBackend();

    // Listen for queue changes from backend
    this.unlistenChanged = await listen<QueueState>(
      "queue-changed",
      (event) => {
        this.updateFromState(event.payload);
      }
    );

    // Listen for playback ended to auto-advance
    // Pass force=false to respect repeat mode (e.g., RepeatMode::One will replay current song)
    if (this.shouldAutoAdvance) {
      this.unlistenEnded = await listen("playback-ended", async () => {
        await this.playNext(undefined, false);
      });
    }

    // Listen for queue ended
    this.unlistenQueueEnded = await listen("queue-ended", () => {
      this.currentIndex = null;
    });
  }

  private updateFromState(state: QueueState) {
    this.items = state.items;
    this.currentIndex = state.current_index;
    this.shuffle = state.shuffle;
    this.repeatMode = state.repeat_mode;
    this.pendingNavigationIndex = state.pending_navigation_index;
  }

  private async loadFromBackend() {
    try {
      const state = await invoke<QueueState>("get_queue");
      this.updateFromState(state);
    } catch (e) {
      error(`Failed to load queue: ${e}`);
    }
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

  async addSong(song: Song) {
    const item = this.songToQueueItem(song);
    try {
      await invoke("add_to_queue", { item });
    } catch (e) {
      error(`Failed to add to queue: ${e}`);
    }
  }

  async addSongs(songs: Song[]) {
    const items = songs.map((s) => this.songToQueueItem(s));
    try {
      await invoke("add_songs_to_queue", { items });
    } catch (e) {
      error(`Failed to add songs to queue: ${e}`);
    }
  }

  async playNext(song?: Song, force: boolean = true) {
    if (song) {
      // Insert song as next to play
      const item = this.songToQueueItem(song);
      try {
        await invoke("insert_next_in_queue", { item });
      } catch (e) {
        error(`Failed to insert next: ${e}`);
      }
    } else {
      // Play the next song in queue
      // force=true: always advance (user clicked Next button)
      // force=false: respect repeat mode (auto-advance when song ends)
      try {
        await invoke<boolean>("play_next", { force });
      } catch (e) {
        error(`Failed to play next: ${e}`);
      }
    }
  }

  async playNextSongs(songs: Song[]) {
    const items = songs.map((s) => this.songToQueueItem(s));
    try {
      await invoke("insert_next_songs_in_queue", { items });
    } catch (e) {
      error(`Failed to insert songs next: ${e}`);
    }
  }

  async playPrevious() {
    try {
      await invoke<boolean>("play_previous");
    } catch (e) {
      error(`Failed to play previous: ${e}`);
    }
  }

  async playQueueItem(index: number) {
    try {
      await invoke("play_queue_item", { index });
    } catch (e) {
      error(`Failed to play queue item: ${e}`);
    }
  }

  async removeFromQueue(index: number) {
    try {
      await invoke("remove_from_queue", { index });
    } catch (e) {
      error(`Failed to remove from queue: ${e}`);
    }
  }

  async clearQueue() {
    try {
      await invoke("clear_queue");
    } catch (e) {
      error(`Failed to clear queue: ${e}`);
    }
  }

  async moveItem(from: number, to: number) {
    try {
      await invoke("move_queue_item", { from, to });
    } catch (e) {
      error(`Failed to move queue item: ${e}`);
    }
  }

  async toggleShuffle() {
    try {
      await invoke<boolean>("toggle_shuffle");
    } catch (e) {
      error(`Failed to toggle shuffle: ${e}`);
    }
  }

  async cycleRepeatMode() {
    try {
      await invoke<RepeatMode>("cycle_repeat_mode");
    } catch (e) {
      error(`Failed to cycle repeat mode: ${e}`);
    }
  }

  async setRepeatMode(mode: RepeatMode) {
    try {
      await invoke("set_repeat_mode", { mode });
    } catch (e) {
      error(`Failed to set repeat mode: ${e}`);
    }
  }

  // Get the current song
  get currentSong(): QueueItem | null {
    if (this.currentIndex === null || this.currentIndex >= this.items.length) {
      return null;
    }
    return this.items[this.currentIndex];
  }

  // Get the previous song (what will play when Previous is clicked)
  get previousSong(): QueueItem | null {
    if (this.items.length === 0) return null;

    // If current was removed, use pending navigation index
    if (this.currentIndex === null && this.pendingNavigationIndex !== null) {
      const prevIdx = this.pendingNavigationIndex - 1;
      if (prevIdx >= 0) {
        return this.items[prevIdx];
      }
      // At start with pending - wrap if repeat all
      if (this.repeatMode === "All") {
        return this.items[this.items.length - 1];
      }
      return this.items[0]; // Stay at beginning
    }

    if (this.currentIndex === null) return null;

    if (this.currentIndex > 0) {
      return this.items[this.currentIndex - 1];
    }

    // At the start - wrap around if repeat is on
    if (this.repeatMode === "All") {
      return this.items[this.items.length - 1];
    }

    return null;
  }

  // Get the next song (what will play when Next is clicked)
  get nextSong(): QueueItem | null {
    if (this.items.length === 0) return null;

    // If current was removed, use pending navigation index
    if (this.currentIndex === null && this.pendingNavigationIndex !== null) {
      const nextIdx = Math.min(
        this.pendingNavigationIndex,
        this.items.length - 1
      );
      return this.items[nextIdx];
    }

    // If repeat one, next will play the same song
    if (this.repeatMode === "One" && this.currentIndex !== null) {
      return this.items[this.currentIndex];
    }

    if (this.currentIndex === null) {
      // Nothing playing, next would start from beginning
      return this.items[0];
    }

    if (this.currentIndex < this.items.length - 1) {
      return this.items[this.currentIndex + 1];
    }

    // At the end - wrap around if repeat is on
    if (this.repeatMode === "All") {
      return this.items[0];
    }

    return null;
  }

  // Check if there's a previous track available
  get hasPrevious(): boolean {
    return this.previousSong !== null;
  }

  // Check if there's a next track available
  get hasNext(): boolean {
    return this.nextSong !== null;
  }

  // Helper to play a song and set up queue
  async playSongWithQueue(song: Song, allSongs?: Song[]) {
    if (allSongs && allSongs.length > 0) {
      await this.clearQueue();
      await this.addSongs(allSongs);

      // Find the index of the song to play
      const index = allSongs.findIndex((s) => s.id === song.id);
      if (index >= 0) {
        await this.playQueueItem(index);
      }
    } else {
      // Just play the single song by adding it and playing
      await this.clearQueue();
      await this.addSong(song);
      await this.playQueueItem(0);
    }
  }

  // Cleanup
  destroy() {
    if (this.unlistenChanged) {
      this.unlistenChanged();
    }
    if (this.unlistenEnded) {
      this.unlistenEnded();
    }
    if (this.unlistenQueueEnded) {
      this.unlistenQueueEnded();
    }
  }
}

export const queue = new QueueStore();
