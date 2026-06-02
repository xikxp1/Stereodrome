import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { error } from "@tauri-apps/plugin-log";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  getCoverArtPath,
  getNotificationSettings,
  sendNowPlayingNotification,
} from "$lib/api/commands";
import type { NotificationSettings } from "$lib/types";

const MINI_PLAYER_LABEL = "mini-player";

class NotificationService {
  private isFocused = $state(true);
  private permissionGranted = $state(false);
  private unlistenFocus: UnlistenFn | null = null;
  private initialized = false;
  private readonly shouldNotify = getCurrentWindow().label === "main";

  async init() {
    if (this.initialized) return;
    this.initialized = true;
    if (!this.shouldNotify) return;

    // Track window focus state
    this.unlistenFocus = await getCurrentWindow().onFocusChanged(
      ({ payload: focused }) => {
        this.isFocused = focused;
      }
    );

    // Get initial focus state
    this.isFocused = await getCurrentWindow().isFocused();
  }

  private async ensurePermissionGranted(): Promise<boolean> {
    if (this.permissionGranted) return true;

    let granted = await isPermissionGranted();
    if (!granted) {
      const permission = await requestPermission();
      granted = permission === "granted";
    }

    this.permissionGranted = granted;
    return granted;
  }

  private async isMiniPlayerVisible(): Promise<boolean> {
    const miniPlayer = await WebviewWindow.getByLabel(MINI_PLAYER_LABEL);
    if (!miniPlayer) return false;

    try {
      return await miniPlayer.isVisible();
    } catch {
      return false;
    }
  }

  async notifySongChange(
    title: string,
    artist: string,
    coverArtId: string | null
  ) {
    if (!this.shouldNotify) return;
    let settings: NotificationSettings;
    try {
      settings = await getNotificationSettings();
    } catch (e) {
      error(`Failed to read notification settings: ${e}`);
      return;
    }

    if (!settings.enabled) {
      return;
    }
    if (!settings.notify_when_focused && this.isFocused) {
      return;
    }
    if (
      !settings.notify_when_miniplayer_open &&
      (await this.isMiniPlayerVisible())
    ) {
      return;
    }
    if (!(await this.ensurePermissionGranted())) {
      return;
    }

    const body = artist ? `${artist} - ${title}` : title;

    let coverArtPath: string | null = null;
    if (coverArtId) {
      try {
        coverArtPath = await getCoverArtPath(coverArtId, 128);
      } catch {
        // Cover art not available, send without artwork
      }
    }

    try {
      const handled = await sendNowPlayingNotification({
        title: "Now Playing",
        body,
        cover_art_path: coverArtPath,
      });
      if (handled) return;
    } catch (e) {
      error(`Failed to send native now playing notification: ${e}`);
    }

    const attachments = coverArtPath
      ? [{ id: "cover", url: `file://${coverArtPath}` }]
      : undefined;

    sendNotification({
      title: "Now Playing",
      body,
      attachments,
    });
  }

  destroy() {
    if (!this.initialized) return;
    if (this.unlistenFocus) {
      this.unlistenFocus();
    }
    this.unlistenFocus = null;
    this.initialized = false;
  }
}

export const notifications = new NotificationService();
