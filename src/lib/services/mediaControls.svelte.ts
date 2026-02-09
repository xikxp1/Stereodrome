import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { playback } from "$lib/stores/playback.svelte";
import { queue } from "$lib/stores/queue.svelte";

interface MediaControlEvent {
  action: string;
}

class MediaControlsService {
  private unlisten: UnlistenFn | null = null;
  private initialized = false;
  private readonly shouldHandleEvents = getCurrentWindow().label === "main";

  async init() {
    if (this.initialized) return;
    this.initialized = true;
    if (!this.shouldHandleEvents) return;

    // Listen for media control events from OS (emitted by backend via souvlaki)
    this.unlisten = await listen<MediaControlEvent>(
      "media-control",
      (event) => {
        switch (event.payload.action) {
          case "play":
            playback.resume();
            break;
          case "pause":
            playback.pause();
            break;
          case "play_pause":
            playback.togglePlayPause();
            break;
          case "next":
            queue.playNext();
            break;
          case "previous":
            queue.playPrevious();
            break;
          case "stop":
            playback.stop();
            break;
        }
      }
    );
  }

  destroy() {
    if (!this.initialized) return;
    this.unlisten?.();
    this.unlisten = null;
    this.initialized = false;
  }
}

export const mediaControls = new MediaControlsService();
