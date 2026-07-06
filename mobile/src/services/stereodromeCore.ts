import { Directory, Paths } from "expo-file-system";
import NativeStereodromeCore from "../../modules/stereodrome-core/src";
import type {
  ConnectionStatus,
  ConnectivitySettings,
  Artist,
  Album,
  AlbumListEntry,
  AudioProcessingSettings,
  AudioPlaybackStatus,
  CacheStats,
  DownloadStatus,
  LibrarySyncStatus,
  LastfmAuthStart,
  LastfmQueueItem,
  LastfmStatus,
  PlaybackProgress,
  PlaybackSnapshot,
  PlaybackStateSnapshot,
  Playlist,
  QueueItem,
  QueueState,
  RepeatMode,
  ScanStatus,
  SearchResults,
  SavedPlaylistOfflineResult,
  SavedPlaylistOfflineStatus,
  Song,
  SyncSettings,
} from "@/types/music";

type Envelope<T> = { ok: true; value: T } | { ok: false; error: string };
type CoreEventName =
  | "playback-snapshot"
  | "sync-status-changed"
  | "error";
type CoreEventHandler<T = unknown> = (payload: T) => void;

const unavailable =
  "Stereodrome native core is not available in this development build";
const syncStatusPollIntervalMs = 2000;

let initializePromise: Promise<boolean> | null = null;
let syncStatusPoll: ReturnType<typeof setInterval> | null = null;
let syncStatusPollInFlight = false;
let nativePlaybackSubscription: { remove(): void } | null = null;
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

function stopSyncStatusPolling() {
  if (!syncStatusPoll) {
    return;
  }
  clearInterval(syncStatusPoll);
  syncStatusPoll = null;
}

async function pollLibrarySyncStatus() {
  if (syncStatusPollInFlight) {
    return;
  }

  syncStatusPollInFlight = true;
  try {
    const status = await invokeJson<LibrarySyncStatus>("getLibrarySyncStatus");
    emitCoreEvent("sync-status-changed", status);
    if (!status.active_job) {
      stopSyncStatusPolling();
    }
  } catch (error) {
    stopSyncStatusPolling();
    emitCoreEvent("error", error);
  } finally {
    syncStatusPollInFlight = false;
  }
}

function startSyncStatusPolling() {
  void pollLibrarySyncStatus();
  if (!syncStatusPoll) {
    syncStatusPoll = setInterval(
      () => void pollLibrarySyncStatus(),
      syncStatusPollIntervalMs
    );
  }
}

function startNativePlaybackSubscription() {
  if (nativePlaybackSubscription || !NativeStereodromeCore.addListener) {
    return;
  }
  nativePlaybackSubscription = NativeStereodromeCore.addListener(
    "playback-snapshot",
    ({ snapshot }) => {
      try {
        emitCoreEvent(
          "playback-snapshot",
          JSON.parse(snapshot) as PlaybackSnapshot
        );
      } catch (error) {
        emitCoreEvent("error", error);
      }
    }
  );
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
    return await invokeJson<QueueState>(name, payload);
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
    startNativePlaybackSubscription();

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
  updateServerSettings(params: {
    url?: string;
    username?: string;
  }): Promise<ConnectionStatus> {
    return invokeJson("updateServerSettings", params);
  },
  restoreSession(): Promise<ConnectionStatus> {
    return invokeJson("restoreSession");
  },
  disconnectServer(): Promise<void> {
    return invokeJson("disconnectServer");
  },
  async syncLibrary(): Promise<void> {
    await invokeJson<void>("syncLibrary");
    startSyncStatusPolling();
  },
  async syncLibraryIncremental(): Promise<void> {
    await invokeJson<void>("syncLibraryIncremental");
    startSyncStatusPolling();
  },
  getSyncSettings(): Promise<SyncSettings> {
    return invokeJson("getSyncSettings");
  },
  setSyncSettings(settings: SyncSettings): Promise<SyncSettings> {
    return invokeJson("setSyncSettings", settings);
  },
  getConnectivitySettings(): Promise<ConnectivitySettings> {
    return invokeJson("getConnectivitySettings");
  },
  setConnectivitySettings(
    settings: ConnectivitySettings
  ): Promise<ConnectivitySettings> {
    return invokeJson("setConnectivitySettings", settings);
  },
  runDueLibrarySync(): Promise<string | null> {
    return invokeJson("runDueLibrarySync");
  },
  getScanStatus(): Promise<ScanStatus> {
    return invokeJson("getScanStatus");
  },
  startScan(): Promise<ScanStatus> {
    return invokeJson("startScan");
  },
  getLibrarySyncStatus(): Promise<LibrarySyncStatus> {
    return invokeJson<LibrarySyncStatus>("getLibrarySyncStatus").then(
      (status) => {
        if (status.active_job) {
          startSyncStatusPolling();
        }
        return status;
      }
    );
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
  getAudioCacheStats(): Promise<CacheStats> {
    return invokeJson("getAudioCacheStats");
  },
  getOfflineSongIds(): Promise<string[]> {
    return invokeJson("getOfflineSongIds");
  },
  setMaxCacheSize(maxSize: number): Promise<CacheStats> {
    return invokeJson("setMaxCacheSize", maxSize);
  },
  clearAudioCache(): Promise<CacheStats> {
    return invokeJson("clearAudioCache");
  },
  isSongCached(songId: string): Promise<DownloadStatus> {
    return invokeJson("isSongCached", songId);
  },
  downloadSong(songId: string): Promise<DownloadStatus> {
    return invokeJson("downloadSong", songId);
  },
  removeCachedSong(songId: string): Promise<DownloadStatus> {
    return invokeJson("removeCachedSong", songId);
  },
  downloadAlbum(albumId: string): Promise<DownloadStatus[]> {
    return invokeJson("downloadAlbum", albumId);
  },
  downloadPlaylist(playlistId: string): Promise<DownloadStatus[]> {
    return invokeJson("downloadPlaylist", playlistId);
  },
  setPlaylistSavedOffline(
    playlistId: string,
    savedOffline: boolean
  ): Promise<SavedPlaylistOfflineResult> {
    return invokeJson("setPlaylistSavedOffline", {
      playlist_id: playlistId,
      saved_offline: savedOffline,
    });
  },
  reconcileSavedPlaylistsOffline(): Promise<SavedPlaylistOfflineResult[]> {
    return invokeJson("reconcileSavedPlaylistsOffline");
  },
  startSavedPlaylistsOfflineReconcile(): Promise<void> {
    return invokeJson("startSavedPlaylistsOfflineReconcile");
  },
  getSavedPlaylistsOfflineReconcileStatus(): Promise<SavedPlaylistOfflineStatus> {
    return invokeJson("getSavedPlaylistsOfflineReconcileStatus");
  },
  prefetchNext(): Promise<DownloadStatus | null> {
    return invokeJson("prefetchNext");
  },
  getPlaybackState(): Promise<PlaybackStateSnapshot> {
    return invokeJson("getPlaybackState");
  },
  getPlaybackSnapshot(): Promise<PlaybackSnapshot> {
    return invokeJson("getPlaybackSnapshot");
  },
  savePlaybackPosition(
    progress: PlaybackProgress
  ): Promise<PlaybackStateSnapshot> {
    return invokeJson("savePlaybackPosition", progress);
  },
  getLastfmStatus(): Promise<LastfmStatus> {
    return invokeJson("getLastfmStatus");
  },
  beginLastfmAuth(): Promise<LastfmAuthStart> {
    return invokeJson("beginLastfmAuth");
  },
  completeLastfmAuth(): Promise<LastfmStatus> {
    return invokeJson("completeLastfmAuth");
  },
  disconnectLastfm(): Promise<LastfmStatus> {
    return invokeJson("disconnectLastfm");
  },
  getLastfmQueue(): Promise<LastfmQueueItem[]> {
    return invokeJson("getLastfmQueue");
  },
  retryLastfmQueue(): Promise<number> {
    return invokeJson("retryLastfmQueue");
  },
  audioPlayCurrent(): Promise<AudioPlaybackStatus> {
    return invokeJson("audioPlayCurrent");
  },
  audioApplySettings(): Promise<AudioPlaybackStatus> {
    return invokeJson("audioApplySettings");
  },
  audioPrepareNextTransition(): Promise<void> {
    return invokeJson("audioPrepareNextTransition");
  },
  audioPause(): Promise<void> {
    return invokeJson("audioPause");
  },
  audioResume(): Promise<void> {
    return invokeJson("audioResume");
  },
  audioRebuildOutput(): Promise<void> {
    return invokeJson("audioRebuildOutput");
  },
  audioStop(): Promise<void> {
    return invokeJson("audioStop");
  },
  audioSeek(positionSeconds: number): Promise<void> {
    return invokeJson("audioSeek", positionSeconds);
  },
  audioSetVolume(volume: number): Promise<void> {
    return invokeJson("audioSetVolume", volume);
  },
  getAudioProcessingSettings(): Promise<AudioProcessingSettings> {
    return invokeJson("getAudioProcessingSettings");
  },
  setAudioProcessingSettings(
    settings: AudioProcessingSettings
  ): Promise<AudioProcessingSettings> {
    return invokeJson("setAudioProcessingSettings", settings);
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
    return (await this.getPlaybackSnapshot()).queue;
  },
  async playNext(force = true): Promise<QueueState> {
    await invokeJson<QueueItem | null>("playNext", force);
    return (await this.getPlaybackSnapshot()).queue;
  },
  async playPrevious(): Promise<QueueState> {
    await invokeJson<QueueItem | null>("playPrevious");
    return (await this.getPlaybackSnapshot()).queue;
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
