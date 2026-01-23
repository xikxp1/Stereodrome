import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { playback } from "$lib/stores/playback.svelte";
import { queue } from "$lib/stores/queue.svelte";

interface TrayControlEvent {
  action: string;
}

class TrayControlsService {
  private unlisten: UnlistenFn | null = null;

  async init() {
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
    this.unlisten?.();
    this.unlisten = null;
  }
}

export const trayControls = new TrayControlsService();
