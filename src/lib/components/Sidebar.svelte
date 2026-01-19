<script lang="ts">
  import { connection } from '$lib/stores/connection.svelte';
  import { syncLibrary } from '$lib/api/commands';
  import { queryClient } from '$lib/db/queryClient';

  let isSyncing = $state(false);
  let syncError = $state<string | null>(null);

  async function handleSync() {
    isSyncing = true;
    syncError = null;
    try {
      const result = await syncLibrary();
      // Invalidate queries to refresh data
      await queryClient.invalidateQueries({ queryKey: ['artists'] });
      await queryClient.invalidateQueries({ queryKey: ['albums'] });
      await queryClient.invalidateQueries({ queryKey: ['songs'] });
      console.log('Sync complete:', result);
    } catch (e) {
      syncError = e instanceof Error ? e.message : String(e);
    } finally {
      isSyncing = false;
    }
  }
</script>

<div class="flex flex-col h-full bg-base-200">
  <div class="p-4 border-b border-base-300">
    <h1 class="text-lg font-bold">Stereodrome</h1>
    {#if connection.status.connected}
      <p class="text-xs opacity-50 truncate">{connection.status.server_url}</p>
    {/if}
  </div>

  <nav class="flex-1 p-2">
    <ul class="menu">
      <li class="menu-title">Library</li>
      <li><a href="/" class="active">Artists</a></li>
      <li><a href="/albums">Albums</a></li>
      <li><a href="/songs">Songs</a></li>
    </ul>
  </nav>

  <div class="p-4 border-t border-base-300 space-y-2">
    <button
      class="btn btn-sm btn-block"
      onclick={handleSync}
      disabled={isSyncing || !connection.status.connected}
    >
      {#if isSyncing}
        <span class="loading loading-spinner loading-xs"></span>
        Syncing...
      {:else}
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
        </svg>
        Sync Library
      {/if}
    </button>
    {#if syncError}
      <p class="text-xs text-error">{syncError}</p>
    {/if}
    <button
      class="btn btn-sm btn-outline btn-error btn-block"
      onclick={() => connection.disconnect()}
    >
      Disconnect
    </button>
  </div>
</div>
