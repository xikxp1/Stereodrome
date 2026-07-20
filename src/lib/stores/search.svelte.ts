import { searchLibrary } from "$lib/api/commands";
import { logError } from "$lib/services/logging";
import type { SearchResults } from "$lib/types";
import { debug } from "@tauri-apps/plugin-log";

class SearchStore {
  query = $state("");
  activeQuery = $state("");
  isSearching = $state(false);
  results = $state<SearchResults | null>(null);

  // Sets of matched IDs for efficient filtering
  matchedSongIds = $state<Set<string>>(new Set());
  matchedAlbumIds = $state<Set<string>>(new Set());
  matchedArtistIds = $state<Set<string>>(new Set());

  private debounceTimeout: ReturnType<typeof setTimeout> | null = null;
  private searchRequestId = 0;
  private readonly DEBOUNCE_MS = 300;

  setQuery(query: string) {
    this.query = query;
    const requestId = ++this.searchRequestId;

    // Clear existing timeout
    if (this.debounceTimeout) {
      clearTimeout(this.debounceTimeout);
    }

    // Debounce the search
    this.debounceTimeout = setTimeout(() => {
      void this.search(requestId);
    }, this.DEBOUNCE_MS);
  }

  private async search(requestId: number) {
    if (requestId !== this.searchRequestId) return;

    const q = this.query.trim();
    if (q === this.activeQuery) {
      this.isSearching = false;
      return;
    }

    if (!q) {
      this.activeQuery = "";
      this.isSearching = false;
      this.results = null;
      this.matchedSongIds = new Set();
      this.matchedAlbumIds = new Set();
      this.matchedArtistIds = new Set();
      return;
    }

    this.isSearching = true;

    try {
      // Call Tantivy backend with high limit to get all matches
      const results = await searchLibrary(q, 1000);
      if (requestId !== this.searchRequestId || this.query.trim() !== q) {
        return;
      }

      this.results = results;

      void debug(
        `Search '${q}': ${results.songs.length} songs, ${results.albums.length} albums, ${results.artists.length} artists`
      );

      // Build ID sets for efficient filtering
      this.matchedSongIds = new Set(results.songs.map((s) => s.id));
      this.matchedAlbumIds = new Set(results.albums.map((a) => a.id));
      this.matchedArtistIds = new Set(results.artists.map((a) => a.id));

      void debug(`Matched IDs: ${this.matchedSongIds.size} song IDs`);

      this.activeQuery = q;
    } catch (cause) {
      if (requestId === this.searchRequestId && this.query.trim() === q) {
        logError("Search failed", cause);
      }
    } finally {
      if (requestId === this.searchRequestId && this.query.trim() === q) {
        this.isSearching = false;
      }
    }
  }

  hasActiveQuery = $derived(this.activeQuery.length > 0);
}

export const searchStore = new SearchStore();
