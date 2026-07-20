import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  getCoverArtPath,
  getNotificationSettings,
  sendNowPlayingNotification,
} from "$lib/api/commands";
import { logError } from "$lib/services/logging";
import type { NotificationSettings } from "$lib/types";

const MINI_PLAYER_LABEL = "mini-player";

class NotificationService {
  private isFocused = $state(true);
  private permissionGranted = $state(false);
  private unlistenFocus: UnlistenFn | null = null;
  private initialized = false;
  private readonly shouldNotify = getCurrentWindow().label === "main";
  private readonly notifiedUpdateVersions = new Set<string>();

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
    } catch (cause) {
      logError("Failed to read notification settings", cause);
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

    const body = artist !== "" ? `${artist} - ${title}` : title;

    let coverArtPath: string | null = null;
    if (coverArtId !== null && coverArtId !== "") {
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
    } catch (cause) {
      logError("Failed to send native now playing notification", cause);
    }

    const attachments =
      coverArtPath !== null && coverArtPath !== ""
        ? [{ id: "cover", url: `file://${coverArtPath}` }]
        : undefined;

    sendNotification({
      title: "Now Playing",
      body,
      ...(attachments ? { attachments } : {}),
    });
  }

  async notifyUpdateAvailable(version: string) {
    if (!this.shouldNotify) return;
    if (this.notifiedUpdateVersions.has(version)) return;

    let settings: NotificationSettings;
    try {
      settings = await getNotificationSettings();
    } catch (cause) {
      logError("Failed to read notification settings", cause);
      return;
    }

    if (!settings.enabled) {
      return;
    }
    if (!(await this.ensurePermissionGranted())) {
      return;
    }

    sendNotification({
      title: "Stereodrome Update Available",
      body: `Version ${version} is available to install.`,
    });
    this.notifiedUpdateVersions.add(version);
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
