import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { error } from "@tauri-apps/plugin-log";
import {
  closeTaskbarWidget,
  getTaskbarWidgetSettings,
  isTaskbarWidgetSupported,
  openTaskbarWidget,
  repositionTaskbarWidget,
} from "$lib/api/commands";
import type { TaskbarWidgetSettings } from "$lib/types";

interface PlaybackStateEvent {
  song: { id: string } | null;
}

class TaskbarWidgetService {
  private initialized = false;
  private supported = false;
  private enabled = false;
  private lastSongId: string | null = null;
  private repositionInterval: ReturnType<typeof setInterval> | null = null;
  private unlistenSettings: UnlistenFn | null = null;
  private unlistenPlayback: UnlistenFn | null = null;
  private readonly shouldManageWidget = getCurrentWindow().label === "main";

  async init() {
    if (this.initialized) return;
    this.initialized = true;
    if (!this.shouldManageWidget) return;

    try {
      this.supported = await isTaskbarWidgetSupported();
    } catch (e) {
      error(`Failed to check taskbar widget support: ${e}`);
      return;
    }

    if (!this.supported) return;

    try {
      const settings = await getTaskbarWidgetSettings();
      await this.applySettings(settings);
    } catch (e) {
      error(`Failed to load taskbar widget settings: ${e}`);
    }

    this.unlistenSettings = await listen<TaskbarWidgetSettings>(
      "taskbar-widget-settings-changed",
      (event) => {
        void this.applySettings(event.payload);
      }
    );

    this.unlistenPlayback = await listen<PlaybackStateEvent>(
      "playback-state",
      (event) => {
        const nextSongId = event.payload.song?.id ?? null;
        if (nextSongId === this.lastSongId) return;
        this.lastSongId = nextSongId;
        void this.reposition();
      }
    );
  }

  private async applySettings(settings: TaskbarWidgetSettings) {
    this.enabled = settings.enabled;

    if (!this.enabled) {
      this.stopRepositionTimer();
      await closeTaskbarWidget().catch((e) => {
        error(`Failed to close taskbar widget: ${e}`);
      });
      return;
    }

    await openTaskbarWidget().catch((e) => {
      error(`Failed to open taskbar widget: ${e}`);
    });
    this.startRepositionTimer();
  }

  private startRepositionTimer() {
    if (this.repositionInterval) return;
    this.repositionInterval = setInterval(() => {
      void this.reposition();
    }, 1500);
  }

  private stopRepositionTimer() {
    if (!this.repositionInterval) return;
    clearInterval(this.repositionInterval);
    this.repositionInterval = null;
  }

  private async reposition() {
    if (!this.enabled || !this.supported) return;
    try {
      await repositionTaskbarWidget();
    } catch (e) {
      error(`Failed to reposition taskbar widget: ${e}`);
    }
  }

  destroy() {
    if (!this.initialized) return;
    this.stopRepositionTimer();
    this.unlistenSettings?.();
    this.unlistenPlayback?.();
    this.unlistenSettings = null;
    this.unlistenPlayback = null;
    this.initialized = false;
  }
}

export const taskbarWidget = new TaskbarWidgetService();
