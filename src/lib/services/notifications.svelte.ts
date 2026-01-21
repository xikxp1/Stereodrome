import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { UnlistenFn } from "@tauri-apps/api/event";

class NotificationService {
  private isFocused = $state(true);
  private permissionGranted = $state(false);
  private unlistenFocus: UnlistenFn | null = null;

  async init() {
    // Check/request notification permission
    let granted = await isPermissionGranted();
    if (!granted) {
      const permission = await requestPermission();
      granted = permission === "granted";
    }
    this.permissionGranted = granted;

    // Track window focus state
    this.unlistenFocus = await getCurrentWindow().onFocusChanged(
      ({ payload: focused }) => {
        this.isFocused = focused;
      }
    );

    // Get initial focus state
    this.isFocused = await getCurrentWindow().isFocused();
  }

  async notifySongChange(
    title: string,
    artist: string,
    coverArtId: string | null
  ) {
    if (!this.permissionGranted || this.isFocused) {
      return;
    }

    const body = artist ? `${artist} - ${title}` : title;

    // Try to get cover art path for attachment
    let attachments: { id: string; url: string }[] | undefined;
    if (coverArtId) {
      try {
        const path = await invoke<string>("get_cover_art_path", {
          coverArtId,
          size: 128,
        });
        attachments = [{ id: "cover", url: `file://${path}` }];
      } catch {
        // Cover art not available, send without attachment
      }
    }

    sendNotification({
      title: "Now Playing",
      body,
      attachments,
    });
  }

  destroy() {
    if (this.unlistenFocus) {
      this.unlistenFocus();
    }
  }
}

export const notifications = new NotificationService();
