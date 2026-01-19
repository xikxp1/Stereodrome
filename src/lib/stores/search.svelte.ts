import { searchLibrary } from "$lib/api/commands";
import type { SearchResults } from "$lib/types";

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
  private readonly DEBOUNCE_MS = 300;

  setQuery(query: string) {
    this.query = query;

    // Clear existing timeout
    if (this.debounceTimeout) {
      clearTimeout(this.debounceTimeout);
    }

    // Debounce the search
    this.debounceTimeout = setTimeout(() => {
      this.search();
    }, this.DEBOUNCE_MS);
  }

  async search() {
    const q = this.query.trim();
    if (q === this.activeQuery) return;

    if (!q) {
      this.clearResults();
      return;
    }

    this.isSearching = true;

    try {
      // Call Tantivy backend with high limit to get all matches
      const results = await searchLibrary(q, 1000);
      this.results = results;

      console.log(
        `Search '${q}': ${results.songs.length} songs, ${results.albums.length} albums, ${results.artists.length} artists`
      );

      // Build ID sets for efficient filtering
      this.matchedSongIds = new Set(results.songs.map((s) => s.id));
      this.matchedAlbumIds = new Set(results.albums.map((a) => a.id));
      this.matchedArtistIds = new Set(results.artists.map((a) => a.id));

      console.log(`Matched IDs: ${this.matchedSongIds.size} song IDs`);

      this.activeQuery = q;
    } catch (e) {
      console.error("Search failed:", e);
      // On error, clear results
      this.clearResults();
    } finally {
      this.isSearching = false;
    }
  }

  private clearResults() {
    this.activeQuery = "";
    this.results = null;
    this.matchedSongIds = new Set();
    this.matchedAlbumIds = new Set();
    this.matchedArtistIds = new Set();
  }

  clear() {
    if (this.debounceTimeout) {
      clearTimeout(this.debounceTimeout);
    }
    this.query = "";
    this.isSearching = false;
    this.clearResults();
  }

  hasQuery = $derived(this.query.trim().length > 0);
  hasActiveQuery = $derived(this.activeQuery.length > 0);
}

export const searchStore = new SearchStore();
