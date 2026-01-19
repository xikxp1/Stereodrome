import { invoke } from "@tauri-apps/api/core";

export interface SearchResultSong {
  id: string;
  title: string;
  artist: string | null;
  album: string | null;
  duration: number | null;
}

export interface SearchResultAlbum {
  id: string;
  name: string;
  artist: string | null;
  year: number | null;
  song_count: number;
}

export interface SearchResultArtist {
  id: string;
  name: string;
  album_count: number;
}

export interface SearchResults {
  songs: SearchResultSong[];
  albums: SearchResultAlbum[];
  artists: SearchResultArtist[];
}

class SearchStore {
  query = $state("");
  results = $state<SearchResults>({ songs: [], albums: [], artists: [] });
  isSearching = $state(false);
  isOpen = $state(false);

  private debounceTimer: ReturnType<typeof setTimeout> | null = null;

  async search(query: string) {
    this.query = query;

    // Clear results if query is empty
    if (!query.trim()) {
      this.results = { songs: [], albums: [], artists: [] };
      this.isOpen = false;
      return;
    }

    // Debounce search
    if (this.debounceTimer) {
      clearTimeout(this.debounceTimer);
    }

    this.debounceTimer = setTimeout(async () => {
      this.isSearching = true;
      try {
        this.results = await invoke<SearchResults>("search_library", {
          query: query.trim(),
          limit: 10,
        });
        this.isOpen = this.hasResults;
      } catch (e) {
        console.error("Search failed:", e);
        this.results = { songs: [], albums: [], artists: [] };
      } finally {
        this.isSearching = false;
      }
    }, 300); // 300ms debounce
  }

  get hasResults() {
    return (
      this.results.songs.length > 0 ||
      this.results.albums.length > 0 ||
      this.results.artists.length > 0
    );
  }

  get totalResults() {
    return (
      this.results.songs.length +
      this.results.albums.length +
      this.results.artists.length
    );
  }

  clear() {
    this.query = "";
    this.results = { songs: [], albums: [], artists: [] };
    this.isOpen = false;
  }

  close() {
    this.isOpen = false;
  }

  open() {
    if (this.hasResults) {
      this.isOpen = true;
    }
  }
}

export const searchStore = new SearchStore();
