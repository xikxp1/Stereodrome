import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { dispatch } from "$lib/api/core";
import {
  playSongWithQueue as playSongWithQueueCommand,
  rerollNextQueueItem,
} from "$lib/api/commands";
import { logError } from "$lib/services/logging";
import type { QueueItem, QueueState, RepeatMode, Song } from "$lib/types";

class QueueStore {
  // State - reflects backend state
  items = $state<QueueItem[]>([]);
  currentIndex = $state<number | null>(null);
  shuffle = $state(false);
  repeatMode = $state<RepeatMode>("Off");
  pendingNavigationIndex = $state<number | null>(null);
  preparedNextItem = $state<QueueItem | null>(null);

  // Event listeners
  private unlistenChanged: UnlistenFn | null = null;

  constructor() {
    void this.init();
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
  }

  private updateFromState(state: QueueState) {
    this.items = state.items;
    this.currentIndex = state.current_index;
    this.shuffle = state.shuffle;
    this.repeatMode = state.repeat_mode;
    this.pendingNavigationIndex = state.pending_navigation_index;
    this.preparedNextItem = state.prepared_next_item;
  }

  private async loadFromBackend() {
    try {
      const state = await dispatch({ type: "get-queue" });
      this.updateFromState(state);
    } catch (cause) {
      logError("Failed to load queue", cause);
    }
  }

  private songToQueueItem(song: Song): QueueItem {
    return {
      song_id: song.id,
      title: song.title,
      artist:
        song.artist !== null && song.artist !== ""
          ? song.artist
          : "Unknown Artist",
      album:
        song.album !== null && song.album !== "" ? song.album : "Unknown Album",
      duration:
        song.duration !== null &&
        song.duration !== 0 &&
        !Number.isNaN(song.duration)
          ? song.duration
          : 0,
    };
  }

  async addSong(song: Song) {
    const item = this.songToQueueItem(song);
    try {
      await dispatch({ type: "add-to-queue", item });
    } catch (cause) {
      logError("Failed to add to queue", cause);
    }
  }

  async addSongs(songs: Song[]) {
    const items = songs.map((s) => this.songToQueueItem(s));
    try {
      await dispatch({ type: "add-songs-to-queue", items });
    } catch (cause) {
      logError("Failed to add songs to queue", cause);
    }
  }

  async playNext(song?: Song, force: boolean = true) {
    if (song) {
      // Insert song as next to play
      const item = this.songToQueueItem(song);
      try {
        await dispatch({ type: "insert-next", item });
      } catch (cause) {
        logError("Failed to insert next", cause);
      }
    } else {
      // Play the next song in queue
      // force=true: always advance (user clicked Next button)
      // force=false: respect repeat mode (auto-advance when song ends)
      try {
        await dispatch({
          type: "navigate-playback",
          navigation: { type: "next", force },
        });
      } catch (cause) {
        logError("Failed to play next", cause);
      }
    }
  }

  async playNextSongs(songs: Song[]) {
    const items = songs.map((s) => this.songToQueueItem(s));
    try {
      await dispatch({ type: "insert-next-songs", items });
    } catch (cause) {
      logError("Failed to insert songs next", cause);
    }
  }

  async playPrevious() {
    try {
      await dispatch({
        type: "navigate-playback",
        navigation: { type: "previous" },
      });
    } catch (cause) {
      logError("Failed to play previous", cause);
    }
  }

  async playQueueItem(index: number) {
    try {
      await dispatch({
        type: "navigate-playback",
        navigation: { type: "index", index },
      });
    } catch (cause) {
      logError("Failed to play queue item", cause);
    }
  }

  async removeFromQueue(index: number) {
    try {
      await dispatch({ type: "remove-from-queue", index });
    } catch (cause) {
      logError("Failed to remove from queue", cause);
    }
  }

  async clearQueue() {
    try {
      await dispatch({ type: "clear-playback" });
    } catch (cause) {
      logError("Failed to clear queue", cause);
    }
  }

  async moveItem(from: number, to: number) {
    try {
      await dispatch({ type: "move-queue-item", from, to });
    } catch (cause) {
      logError("Failed to move queue item", cause);
    }
  }

  async toggleShuffle() {
    try {
      await dispatch({ type: "toggle-shuffle" });
    } catch (cause) {
      logError("Failed to toggle shuffle", cause);
    }
  }

  async cycleRepeatMode() {
    try {
      await dispatch({ type: "cycle-repeat-mode" });
    } catch (cause) {
      logError("Failed to cycle repeat mode", cause);
    }
  }

  async setRepeatMode(mode: RepeatMode) {
    try {
      await dispatch({ type: "set-repeat-mode", mode });
    } catch (cause) {
      logError("Failed to set repeat mode", cause);
    }
  }

  async rerollNext(): Promise<boolean> {
    if (!this.canRerollNext) {
      return false;
    }

    try {
      await rerollNextQueueItem();
      return true;
    } catch (cause) {
      logError("Failed to reroll next track", cause);
      return false;
    }
  }

  // Get the current song
  get currentSong(): QueueItem | null {
    if (this.currentIndex === null || this.currentIndex >= this.items.length) {
      return null;
    }
    return this.items[this.currentIndex] ?? null;
  }

  // Get the previous song (what will play when Previous is clicked)
  get previousSong(): QueueItem | null {
    if (this.items.length === 0) return null;

    // If current was removed, use pending navigation index
    if (this.currentIndex === null && this.pendingNavigationIndex !== null) {
      const prevIdx = this.pendingNavigationIndex - 1;
      if (prevIdx >= 0) {
        return this.items[prevIdx] ?? null;
      }
      // At start with pending - wrap if repeat all
      if (this.repeatMode === "All") {
        return this.items[this.items.length - 1] ?? null;
      }
      return this.items[0] ?? null; // Stay at beginning
    }

    if (this.currentIndex === null) return null;

    if (this.currentIndex > 0) {
      return this.items[this.currentIndex - 1] ?? null;
    }

    // At the start - wrap around if repeat is on
    if (this.repeatMode === "All") {
      return this.items[this.items.length - 1] ?? null;
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
      return this.items[nextIdx] ?? null;
    }

    if (this.preparedNextItem !== null) {
      return this.preparedNextItem;
    }

    // If repeat one, next will play the same song
    if (this.repeatMode === "One" && this.currentIndex !== null) {
      return this.items[this.currentIndex] ?? null;
    }

    if (this.currentIndex === null) {
      // Nothing playing, next would start from beginning
      return this.items[0] ?? null;
    }

    if (this.currentIndex < this.items.length - 1) {
      return this.items[this.currentIndex + 1] ?? null;
    }

    // At the end - wrap around if repeat is on
    if (this.repeatMode === "All") {
      return this.items[0] ?? null;
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

  // Check if reroll can swap the next track with another queue item
  get canRerollNext(): boolean {
    return (
      this.items.length > 2 &&
      this.currentSong !== null &&
      this.nextSong !== null
    );
  }

  // Helper to play a song and set up queue
  async playSongWithQueue(song: Song, allSongs?: Song[]) {
    if (allSongs && allSongs.length > 0) {
      await playSongWithQueueCommand(
        song.id,
        allSongs.map((entry) => entry.id)
      );
    } else {
      await playSongWithQueueCommand(song.id, [song.id]);
    }
  }

  // Cleanup
  destroy() {
    if (this.unlistenChanged) {
      this.unlistenChanged();
    }
  }
}

export const queue = new QueueStore();
