import { getAlbumList, getAlbumCount } from "$lib/api/commands";
import type { AlbumListEntry } from "$lib/types";

const PAGE_SIZE = 120;

class AlbumListStore {
  entries = $state<AlbumListEntry[]>([]);
  totalCount = $state<number>(0);
  isLoading = $state(false);
  isLoadingMore = $state(false);
  error = $state<Error | null>(null);
  hasMore = $state(true);
  currentListType: string | null = null;
  private offset = 0;

  static getListType(activeView: string): string | null {
    switch (activeView) {
      case "recently_added":
        return "newest";
      case "recently_played":
        return "recent";
      case "most_played":
        return "frequent";
      default:
        return null;
    }
  }

  async loadView(activeView: string) {
    const listType = AlbumListStore.getListType(activeView);
    if (!listType) return;

    // Reset all state
    this.entries = [];
    this.totalCount = 0;
    this.offset = 0;
    this.hasMore = true;
    this.error = null;
    this.currentListType = listType;
    this.isLoading = true;
    this.isLoadingMore = false;

    try {
      const [data, count] = await Promise.all([
        getAlbumList(listType, PAGE_SIZE, 0),
        getAlbumCount(),
      ]);
      this.entries = data;
      this.totalCount = count;
      this.offset = data.length;
      this.hasMore = data.length < count;
    } catch (e) {
      this.error = e instanceof Error ? e : new Error(String(e));
    } finally {
      this.isLoading = false;
    }
  }

  async loadMore() {
    if (!this.currentListType || this.isLoadingMore || !this.hasMore) return;

    this.isLoadingMore = true;
    try {
      const data = await getAlbumList(
        this.currentListType,
        PAGE_SIZE,
        this.offset
      );
      this.entries = [...this.entries, ...data];
      this.offset += data.length;
      this.hasMore = this.offset < this.totalCount;
    } catch (e) {
      this.error = e instanceof Error ? e : new Error(String(e));
    } finally {
      this.isLoadingMore = false;
    }
  }
}

export const albumListStore = new AlbumListStore();
