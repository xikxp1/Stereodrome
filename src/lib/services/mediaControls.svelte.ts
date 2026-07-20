import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { seekPlayback } from "$lib/api/commands";
import { logError } from "$lib/services/logging";
import { playback } from "$lib/stores/playback.svelte";
import { queue } from "$lib/stores/queue.svelte";

type MediaControlEvent =
  | { action: "play" }
  | { action: "pause" }
  | { action: "play_pause" }
  | { action: "next" }
  | { action: "previous" }
  | { action: "stop" }
  | { action: "seek"; position: number }
  | { action: "seek_by"; delta: number };

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
        void this.handleEvent(event.payload);
      }
    );
  }

  private async handleEvent(event: MediaControlEvent) {
    switch (event.action) {
      case "play":
        await playback.resume();
        break;
      case "pause":
        await playback.pause();
        break;
      case "play_pause":
        await playback.togglePlayPause();
        break;
      case "next":
        await queue.playNext();
        break;
      case "previous":
        await queue.playPrevious();
        break;
      case "stop":
        await playback.stop();
        break;
      case "seek":
        await this.seekTo(event.position);
        break;
      case "seek_by":
        await this.seekTo(playback.position + event.delta);
        break;
    }
  }

  private async seekTo(position: number) {
    const upperBound = playback.duration > 0 ? playback.duration : position;
    const clampedPosition = Math.max(0, Math.min(upperBound, position));

    try {
      await seekPlayback(clampedPosition);
    } catch (cause) {
      logError("Failed to seek from media controls", cause);
    }
  }

  destroy() {
    if (!this.initialized) return;
    this.unlisten?.();
    this.unlisten = null;
    this.initialized = false;
  }
}

export const mediaControls = new MediaControlsService();
