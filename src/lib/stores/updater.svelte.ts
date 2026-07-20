import { check, type Update } from "@tauri-apps/plugin-updater";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { setTrayUpdateAvailable } from "$lib/api/commands";
import { logError } from "$lib/services/logging";
import { notifications } from "$lib/services/notifications.svelte";

class UpdaterStore {
  updateAvailable = $state(false);
  checking = $state(false);
  downloading = $state(false);
  error = $state<string | null>(null);

  // Update info
  version = $state<string | null>(null);
  notes = $state<string | null>(null);
  currentVersion = $state<string | null>(null);

  // Internal reference to the update object
  private update: Update | null = null;

  async loadCurrentVersion(): Promise<void> {
    try {
      this.currentVersion = await getVersion();
    } catch (cause) {
      logError("Failed to get app version", cause);
    }
  }

  async checkForUpdate(): Promise<boolean> {
    this.checking = true;
    this.error = null;

    try {
      const update = await check();

      if (update) {
        this.update = update;
        this.updateAvailable = true;
        this.version = update.version;
        this.notes = update.body ?? null;
        // Update tray indicator
        void setTrayUpdateAvailable(update.version).catch((cause: unknown) => {
          logError("Failed to set tray update available", cause);
        });
        void notifications
          .notifyUpdateAvailable(update.version)
          .catch((cause: unknown) => {
            logError("Failed to send update available notification", cause);
          });
        return true;
      }

      this.updateAvailable = false;
      this.version = null;
      this.notes = null;
      this.update = null;
      // Clear tray indicator
      void setTrayUpdateAvailable(null).catch((cause: unknown) => {
        logError("Failed to clear tray update indicator", cause);
      });
      return false;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
      return false;
    } finally {
      this.checking = false;
    }
  }

  async downloadAndInstall(): Promise<void> {
    if (!this.update) {
      this.error = "No update available";
      return;
    }

    this.downloading = true;
    this.error = null;

    try {
      await this.update.downloadAndInstall();
      // Relaunch the app after successful installation
      await relaunch();
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.downloading = false;
    }
  }
}

export const updater = new UpdaterStore();
