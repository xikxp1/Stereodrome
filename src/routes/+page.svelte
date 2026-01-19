<script lang="ts">
  import ServerConnect from "$lib/components/ServerConnect.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ArtistList from "$lib/components/library/ArtistList.svelte";
  import AlbumGrid from "$lib/components/library/AlbumGrid.svelte";
  import SongList from "$lib/components/library/SongList.svelte";
  import { connection } from "$lib/stores/connection.svelte";
  import type { Artist, Album } from "$lib/types";

  let selectedArtist = $state<Artist | null>(null);
  let selectedAlbum = $state<Album | null>(null);

  function handleArtistSelect(artist: Artist) {
    selectedArtist = artist;
    selectedAlbum = null;
  }

  function handleAlbumSelect(album: Album) {
    selectedAlbum = album;
  }
</script>

<div class="h-screen bg-base-100 flex flex-col">
  {#if connection.status.connected}
    <div class="flex-1 flex overflow-hidden">
      <!-- Sidebar -->
      <aside class="w-56 border-r border-base-300 flex-shrink-0">
        <Sidebar />
      </aside>

      <!-- Main content -->
      <main class="flex-1 flex overflow-hidden">
        <!-- Artist list -->
        <div class="w-64 border-r border-base-300 flex-shrink-0">
          <ArtistList
            onSelect={handleArtistSelect}
            selectedId={selectedArtist?.id}
          />
        </div>

        <!-- Album/Song area -->
        <div class="flex-1 flex flex-col overflow-hidden">
          {#if selectedAlbum}
            <!-- Show songs for selected album -->
            <SongList albumId={selectedAlbum.id} />
          {:else}
            <!-- Show albums for selected artist -->
            <AlbumGrid
              artistId={selectedArtist?.id}
              onSelect={handleAlbumSelect}
            />
          {/if}
        </div>
      </main>
    </div>

    <!-- Now playing bar placeholder -->
    <footer
      class="h-20 border-t border-base-300 bg-base-200 flex items-center px-4"
    >
      <div class="flex-1 text-sm opacity-50">No track playing</div>
    </footer>
  {:else}
    <!-- Not connected - show login form -->
    <div class="flex-1 flex items-center justify-center p-4">
      <div class="text-center">
        <h1 class="text-4xl font-bold mb-2">Stereodrome</h1>
        <p class="text-base-content/70 mb-8">Your music, everywhere</p>
        <ServerConnect />
      </div>
    </div>
  {/if}
</div>
