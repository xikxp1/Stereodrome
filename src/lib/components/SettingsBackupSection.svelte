<script lang="ts">
  import { Database, Download, RefreshCw } from "lucide-svelte";
  import { ask, open, save } from "@tauri-apps/plugin-dialog";
  import { error } from "@tauri-apps/plugin-log";

  import {
    exportPortableBackup,
    importPortableBackup,
  } from "$lib/api/commands";
  import { refreshLibraryViews } from "$lib/services/libraryRefresh.svelte";
  import { connection } from "$lib/stores/connection.svelte";
  import { playlistStore } from "$lib/stores/playlist.svelte";
  import { searchStore } from "$lib/stores/search.svelte";

  interface Props {
    onImported: () => Promise<void>;
  }

  let { onImported }: Props = $props();
  let exporting = $state(false);
  let importing = $state(false);
  let message = $state<string | null>(null);

  async function handleExport() {
    const date = new Date().toISOString().slice(0, 10);
    const path = await save({
      title: "Export Stereodrome Backup",
      defaultPath: `stereodrome-backup-${date}.json`,
      filters: [{ name: "Stereodrome Backup", extensions: ["json"] }],
    });
    if (!path) return;

    exporting = true;
    message = null;
    try {
      const summary = await exportPortableBackup(path);
      message = `Exported ${summary.songs.toLocaleString()} songs`;
    } catch (cause) {
      message = cause instanceof Error ? cause.message : String(cause);
      error(`Failed to export backup: ${cause}`);
    } finally {
      exporting = false;
    }
  }

  async function handleImport() {
    const selected = await open({
      multiple: false,
      directory: false,
      title: "Import Stereodrome Backup",
      filters: [{ name: "Stereodrome Backup", extensions: ["json"] }],
    });
    if (!selected || Array.isArray(selected)) return;

    const confirmed = await ask(
      "Replace local library metadata, playlists, queue, and preferences with this backup? Accounts and cached media are preserved.",
      { title: "Import Backup", kind: "warning" }
    );
    if (!confirmed) return;

    importing = true;
    message = null;
    try {
      const summary = await importPortableBackup(selected);
      message = `Imported ${summary.songs.toLocaleString()} songs`;
      try {
        await playlistStore.selectPlaylist(null);
        await playlistStore.loadPlaylists();
        searchStore.setQuery("");
        await connection.checkStatus();
        await refreshLibraryViews();
        await onImported();
      } catch (refreshError) {
        message = `Imported ${summary.songs.toLocaleString()} songs; restart to refresh all views`;
        error(
          `Backup imported but views could not be refreshed: ${refreshError}`
        );
      }
    } catch (cause) {
      message = cause instanceof Error ? cause.message : String(cause);
      error(`Failed to import backup: ${cause}`);
    } finally {
      importing = false;
    }
  }
</script>

<div class="rounded-lg border border-base-300 bg-base-200/50 p-4">
  <div class="mb-3 flex items-center gap-2">
    <Database class="h-4 w-4 text-base-content/60" />
    <h3 class="font-medium">Backup &amp; Transfer</h3>
  </div>

  <p class="text-sm text-base-content/60">
    Move library metadata, playlists, queue, and preferences between devices
    without downloading the library again. Accounts, tokens, and cached media
    are not included.
  </p>

  {#if message}
    <div class="mt-3 text-sm text-base-content/70">{message}</div>
  {/if}

  <div class="mt-4 flex flex-wrap gap-2 border-t border-base-300 pt-4">
    <button
      type="button"
      class="btn btn-sm btn-ghost gap-1"
      onclick={handleExport}
      disabled={exporting || importing}
    >
      {#if exporting}
        <RefreshCw class="h-3.5 w-3.5 animate-spin" />
        Exporting...
      {:else}
        <Download class="h-3.5 w-3.5" />
        Export Backup
      {/if}
    </button>
    <button
      type="button"
      class="btn btn-sm btn-warning btn-outline gap-1"
      onclick={handleImport}
      disabled={exporting || importing}
    >
      {#if importing}
        <RefreshCw class="h-3.5 w-3.5 animate-spin" />
        Importing...
      {:else}
        <Database class="h-3.5 w-3.5" />
        Import Backup
      {/if}
    </button>
  </div>
</div>
