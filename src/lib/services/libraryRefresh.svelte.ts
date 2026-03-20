import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { error } from "@tauri-apps/plugin-log";
import { queryClient } from "$lib/db/queryClient";
import type { LibraryContentUpdatedEvent } from "$lib/types";

export const LIBRARY_REFRESHED_EVENT = "library-refreshed";

export async function refreshLibraryViews(): Promise<void> {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["artists"] }),
    queryClient.invalidateQueries({ queryKey: ["albums"] }),
    queryClient.invalidateQueries({ queryKey: ["songs"] }),
  ]);

  window.dispatchEvent(new CustomEvent(LIBRARY_REFRESHED_EVENT));
}

class LibraryRefreshService {
  private unlisten: UnlistenFn | null = null;
  private initialized = false;
  private readonly shouldHandleEvents = getCurrentWindow().label === "main";

  async init() {
    if (this.initialized) return;
    this.initialized = true;
    if (!this.shouldHandleEvents) return;

    this.unlisten = await listen<LibraryContentUpdatedEvent>(
      "library-content-updated",
      async (event) => {
        if (!event.payload.has_new_items) return;

        try {
          await refreshLibraryViews();
        } catch (e) {
          error(`Failed to refresh library views after sync event: ${e}`);
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

export const libraryRefresh = new LibraryRefreshService();
