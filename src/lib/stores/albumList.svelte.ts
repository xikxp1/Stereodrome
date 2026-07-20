import { getAlbumList, getAlbumCount } from "$lib/api/commands";
import type { AlbumListEntry } from "$lib/types";

const PAGE_SIZE = 120;

interface LoadViewOptions {
  force?: boolean;
}

class AlbumListStore {
  entries = $state<AlbumListEntry[]>([]);
  totalCount = $state<number>(0);
  isLoading = $state(false);
  isLoadingMore = $state(false);
  error = $state<Error | null>(null);
  hasMore = $state(true);
  currentListType: string | null = null;
  private offset = 0;
  private loadRequestId = 0;

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

  async loadView(activeView: string, options: LoadViewOptions = {}) {
    const listType = AlbumListStore.getListType(activeView);
    if (listType === null) return;

    const isSameList = this.currentListType === listType;
    if (
      isSameList &&
      options.force !== true &&
      (this.entries.length > 0 || this.isLoading)
    ) {
      return;
    }

    const shouldReset = !isSameList || this.entries.length === 0;
    const requestId = ++this.loadRequestId;

    if (shouldReset) {
      this.entries = [];
      this.totalCount = 0;
      this.offset = 0;
      this.hasMore = true;
    }

    this.error = null;
    this.currentListType = listType;
    this.isLoading = true;
    this.isLoadingMore = false;

    try {
      const [data, count] = await Promise.all([
        getAlbumList(listType, PAGE_SIZE, 0),
        getAlbumCount(),
      ]);
      if (this.loadRequestId !== requestId || this.currentListType !== listType)
        return;

      this.entries = data;
      this.totalCount = count;
      this.offset = data.length;
      this.hasMore = data.length < count;
    } catch (e) {
      if (
        this.loadRequestId === requestId &&
        this.currentListType === listType
      ) {
        this.error = e instanceof Error ? e : new Error(String(e));
      }
    } finally {
      if (
        this.loadRequestId === requestId &&
        this.currentListType === listType
      ) {
        this.isLoading = false;
      }
    }
  }

  async loadMore() {
    if (this.currentListType === null || this.isLoadingMore || !this.hasMore)
      return;

    const listType = this.currentListType;
    const requestId = this.loadRequestId;

    this.isLoadingMore = true;
    try {
      const data = await getAlbumList(listType, PAGE_SIZE, this.offset);
      if (this.loadRequestId !== requestId || this.currentListType !== listType)
        return;

      this.entries = [...this.entries, ...data];
      this.offset += data.length;
      this.hasMore = this.offset < this.totalCount;
    } catch (e) {
      if (
        this.loadRequestId === requestId &&
        this.currentListType === listType
      ) {
        this.error = e instanceof Error ? e : new Error(String(e));
      }
    } finally {
      if (
        this.loadRequestId === requestId &&
        this.currentListType === listType
      ) {
        this.isLoadingMore = false;
      }
    }
  }
}

export const albumListStore = new AlbumListStore();
