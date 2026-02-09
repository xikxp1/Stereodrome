import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { playback } from "$lib/stores/playback.svelte";
import { queue } from "$lib/stores/queue.svelte";

interface TrayControlEvent {
  action: string;
}

class TrayControlsService {
  private unlisten: UnlistenFn | null = null;
  private initialized = false;
  private readonly shouldHandleEvents = getCurrentWindow().label === "main";

  async init() {
    if (this.initialized) return;
    this.initialized = true;
    if (!this.shouldHandleEvents) return;

    // Listen for tray control events (emitted by backend tray menu)
    this.unlisten = await listen<TrayControlEvent>("tray-control", (event) => {
      switch (event.payload.action) {
        case "play_pause":
          playback.togglePlayPause();
          break;
        case "next":
          queue.playNext();
          break;
        case "previous":
          queue.playPrevious();
          break;
        case "open_settings":
          // Dispatch custom event for +page.svelte to handle
          window.dispatchEvent(new CustomEvent("open-settings"));
          break;
      }
    });
  }

  destroy() {
    if (!this.initialized) return;
    this.unlisten?.();
    this.unlisten = null;
    this.initialized = false;
  }
}

export const trayControls = new TrayControlsService();
