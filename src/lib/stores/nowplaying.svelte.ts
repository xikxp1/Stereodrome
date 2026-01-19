import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { NowPlayingEntry, NowPlayingEvent } from "$lib/types";

class NowPlayingStore {
  // State
  entries = $state<NowPlayingEntry[]>([]);

  // Event listener
  private unlistenNowPlaying: UnlistenFn | null = null;

  constructor() {
    this.setupEventListeners();
  }

  private async setupEventListeners() {
    // Listen for now playing updates from the server
    this.unlistenNowPlaying = await listen<NowPlayingEvent>(
      "now-playing",
      (event) => {
        this.entries = event.payload.entries;
      }
    );
  }

  // Cleanup
  destroy() {
    if (this.unlistenNowPlaying) {
      this.unlistenNowPlaying();
    }
  }
}

export const nowPlaying = new NowPlayingStore();
