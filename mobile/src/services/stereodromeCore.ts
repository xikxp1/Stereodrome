import { Directory, Paths } from "expo-file-system";
import NativeStereodromeCore from "../../modules/stereodrome-core/src";
import type {
  ConnectionStatus,
  Artist,
  Album,
  AlbumListEntry,
  LibrarySyncStatus,
  Playlist,
  QueueItem,
  QueueState,
  RepeatMode,
  SearchResults,
  Song,
  SyncResult,
} from "@/types/music";

type Envelope<T> = { ok: true; value: T } | { ok: false; error: string };
type CoreEventName =
  | "queue-changed"
  | "playback-state"
  | "sync-status-changed"
  | "error";
type CoreEventHandler<T = unknown> = (payload: T) => void;

const unavailable =
  "Stereodrome native core is not available in this development build";

let initializePromise: Promise<boolean> | null = null;
const listeners = new Map<CoreEventName, Set<CoreEventHandler>>();

function fileUriToPath(uri: string): string {
  if (!uri.startsWith("file://")) {
    return uri;
  }

  return decodeURIComponent(uri.replace(/^file:\/\//, ""));
}

function parseEnvelope<T>(raw: string): T {
  const envelope = JSON.parse(raw) as Envelope<T>;
  if (!envelope.ok) {
    throw new Error(envelope.error);
  }
  return envelope.value;
}

async function invokeJson<T>(
  name: string,
  payload: unknown = null
): Promise<T> {
  await ensureInitialized();

  if (!NativeStereodromeCore?.call) {
    throw new Error(unavailable);
  }
  return parseEnvelope<T>(
    await NativeStereodromeCore.call(name, JSON.stringify(payload))
  );
}

function emitCoreEvent<T>(name: CoreEventName, payload: T) {
  listeners.get(name)?.forEach((listener) => listener(payload));
}

function queueItemFromSong(song: {
  id: string;
  title: string;
  artist: string | null;
  album: string | null;
  duration: number | null;
}): QueueItem {
  return {
    song_id: song.id,
    title: song.title,
    artist: song.artist ?? "Unknown Artist",
    album: song.album ?? "Unknown Album",
    duration: song.duration ?? 0,
  };
}

async function queueMutation(
  name: string,
  payload: unknown = null
): Promise<QueueState> {
  try {
    const state = await invokeJson<QueueState>(name, payload);
    emitCoreEvent("queue-changed", state);
    return state;
  } catch (error) {
    emitCoreEvent("error", error);
    throw error;
  }
}

async function ensureInitialized(): Promise<boolean> {
  if (initializePromise) {
    return initializePromise;
  }

  initializePromise = (async () => {
    if (!NativeStereodromeCore?.initialize) {
      throw new Error(unavailable);
    }

    const dataDir = new Directory(Paths.document, "stereodrome");
    dataDir.create({ idempotent: true, intermediates: true });

    const initialized = await NativeStereodromeCore.initialize(
      fileUriToPath(dataDir.uri)
    );
    if (!initialized) {
      throw new Error("Stereodrome Rust core failed to initialize");
    }

    return true;
  })();

  try {
    return await initializePromise;
  } catch (error) {
    initializePromise = null;
    throw error;
  }
}

export const stereodromeCore = {
  initialize: ensureInitialized,
  addEventListener<T = unknown>(
    name: CoreEventName,
    listener: CoreEventHandler<T>
  ): () => void {
    const current = listeners.get(name) ?? new Set<CoreEventHandler>();
    current.add(listener as CoreEventHandler);
    listeners.set(name, current);
    return () => {
      current.delete(listener as CoreEventHandler);
    };
  },
  getConnectionStatus(): Promise<ConnectionStatus> {
    return invokeJson("getConnectionStatus");
  },
  connectServer(params: {
    url: string;
    username: string;
    password: string;
  }): Promise<ConnectionStatus> {
    return invokeJson("connectServer", params);
  },
  restoreSession(): Promise<ConnectionStatus> {
    return invokeJson("restoreSession");
  },
  disconnectServer(): Promise<void> {
    return invokeJson("disconnectServer");
  },
  syncLibrary(): Promise<SyncResult> {
    return invokeJson<SyncResult>("syncLibrary").then((result) => {
      emitCoreEvent("sync-status-changed", result);
      return result;
    });
  },
  syncLibraryIncremental(): Promise<SyncResult> {
    return invokeJson<SyncResult>("syncLibraryIncremental").then((result) => {
      emitCoreEvent("sync-status-changed", result);
      return result;
    });
  },
  reconcileLibrary(): Promise<SyncResult> {
    return invokeJson<SyncResult>("reconcileLibrary").then((result) => {
      emitCoreEvent("sync-status-changed", result);
      return result;
    });
  },
  getLibrarySyncStatus(): Promise<LibrarySyncStatus> {
    return invokeJson("getLibrarySyncStatus");
  },
  getArtists(): Promise<Artist[]> {
    return invokeJson("getArtists");
  },
  getAlbums(artistId?: string): Promise<Album[]> {
    return invokeJson("getAlbums", artistId ?? null);
  },
  getAlbumList(
    listType: string,
    size = 50,
    offset = 0
  ): Promise<AlbumListEntry[]> {
    return invokeJson("getAlbumList", { list_type: listType, size, offset });
  },
  getSongs(albumId?: string, artistId?: string): Promise<Song[]> {
    return invokeJson("getSongs", {
      first: albumId ?? null,
      second: artistId ?? null,
    });
  },
  getPlaylists(): Promise<Playlist[]> {
    return invokeJson("getPlaylists");
  },
  getPlaylistSongs(id: string): Promise<Song[]> {
    return invokeJson("getPlaylistSongs", id);
  },
  createPlaylist(name: string, songIds: string[] = []): Promise<Playlist> {
    return invokeJson("createPlaylist", { name, song_ids: songIds });
  },
  renamePlaylist(playlistId: string, name: string): Promise<void> {
    return invokeJson("renamePlaylist", { playlist_id: playlistId, name });
  },
  deletePlaylist(playlistId: string): Promise<void> {
    return invokeJson("deletePlaylist", playlistId);
  },
  addSongsToPlaylist(playlistId: string, songIds: string[]): Promise<void> {
    return invokeJson("addSongsToPlaylist", {
      playlist_id: playlistId,
      song_ids: songIds,
    });
  },
  removeSongsFromPlaylist(
    playlistId: string,
    songIndexes: number[]
  ): Promise<void> {
    return invokeJson("removeSongsFromPlaylist", {
      playlist_id: playlistId,
      song_indexes: songIndexes,
    });
  },
  searchLibrary(query: string, limit = 25): Promise<SearchResults> {
    return invokeJson("searchLibrary", { query, limit });
  },
  getCoverArtUri(coverArtId: string, size = 512): Promise<string> {
    return invokeJson("getCoverArtUri", { id: coverArtId, size });
  },
  getSongCoverArtUri(songId: string, size = 512): Promise<string | null> {
    return invokeJson("getSongCoverArtUri", { id: songId, size });
  },
  getStreamUri(songId: string): Promise<string> {
    return invokeJson("getStreamUri", songId);
  },
  getQueue(): Promise<QueueState> {
    return invokeJson("getQueue");
  },
  playSongWithQueue(songId: string, songIds: string[]): Promise<QueueState> {
    return queueMutation("playSongWithQueue", {
      song_id: songId,
      song_ids: songIds,
    });
  },
  addToQueue(
    song: Parameters<typeof queueItemFromSong>[0]
  ): Promise<QueueState> {
    return queueMutation("addToQueue", queueItemFromSong(song));
  },
  addSongsToQueue(
    songs: Parameters<typeof queueItemFromSong>[0][]
  ): Promise<QueueState> {
    return queueMutation("addSongsToQueue", songs.map(queueItemFromSong));
  },
  insertNext(
    song: Parameters<typeof queueItemFromSong>[0]
  ): Promise<QueueState> {
    return queueMutation("insertNext", queueItemFromSong(song));
  },
  insertNextSongs(
    songs: Parameters<typeof queueItemFromSong>[0][]
  ): Promise<QueueState> {
    return queueMutation("insertNextSongs", songs.map(queueItemFromSong));
  },
  removeFromQueue(index: number): Promise<QueueState> {
    return queueMutation("removeFromQueue", index);
  },
  clearQueue(): Promise<QueueState> {
    return queueMutation("clearQueue");
  },
  moveQueueItem(from: number, to: number): Promise<QueueState> {
    return queueMutation("moveQueueItem", { from, to });
  },
  async playQueueItem(index: number): Promise<QueueState> {
    await invokeJson<QueueItem | null>("playQueueItem", index);
    const state = await this.getQueue();
    emitCoreEvent("queue-changed", state);
    return state;
  },
  async playNext(force = true): Promise<QueueState> {
    await invokeJson<QueueItem | null>("playNext", force);
    const state = await this.getQueue();
    emitCoreEvent("queue-changed", state);
    return state;
  },
  async playPrevious(): Promise<QueueState> {
    await invokeJson<QueueItem | null>("playPrevious");
    const state = await this.getQueue();
    emitCoreEvent("queue-changed", state);
    return state;
  },
  toggleShuffle(): Promise<QueueState> {
    return queueMutation("toggleShuffle");
  },
  setRepeatMode(mode: RepeatMode): Promise<QueueState> {
    return queueMutation("setRepeatMode", mode);
  },
  cycleRepeatMode(): Promise<QueueState> {
    return queueMutation("cycleRepeatMode");
  },
  rerollNext(): Promise<QueueState> {
    return queueMutation("rerollNext");
  },
};
