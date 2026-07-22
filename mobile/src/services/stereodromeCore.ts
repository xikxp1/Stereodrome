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
  BackupSummary,
  CacheStats,
  DownloadStatus,
  FileStateSnapshot,
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
type PayloadValidator<T> = (value: unknown) => value is T;

const unavailable =
  "Stereodrome native core is not available in this development build";

let initializePromise: Promise<boolean> | null = null;
let nextRuntimeCommandId = Math.floor(Date.now() * 1000);

function fileUriToPath(uri: string): string {
  if (!uri.startsWith("file://")) {
    return uri;
  }

  return decodeURIComponent(uri.replace(/^file:\/\//, ""));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isEnvelope(value: unknown): value is Envelope<unknown> {
  if (!isRecord(value) || typeof value["ok"] !== "boolean") {
    return false;
  }
  return value["ok"] ? "value" in value : typeof value["error"] === "string";
}

function parseEnvelope<T>(
  raw: string,
  isPayload: (value: unknown) => value is T
): T {
  const envelope: unknown = JSON.parse(raw);
  if (!isEnvelope(envelope)) {
    throw new Error("Native core returned an invalid response envelope");
  }
  if (!envelope.ok) {
    throw new Error(envelope.error);
  }
  if (!isPayload(envelope.value)) {
    throw new Error("Native core returned an invalid response payload");
  }
  return envelope.value;
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isNullableNumber(value: unknown): value is number | null {
  return value === null || typeof value === "number";
}

function isNull(value: unknown): value is null {
  return value === null;
}

function isString(value: unknown): value is string {
  return typeof value === "string";
}

function isNumber(value: unknown): value is number {
  return typeof value === "number";
}

function isNullable<T>(
  validator: PayloadValidator<T>
): PayloadValidator<T | null> {
  return (value): value is T | null => value === null || validator(value);
}

function isArrayOf<T>(validator: PayloadValidator<T>): PayloadValidator<T[]> {
  return (value): value is T[] =>
    Array.isArray(value) && value.every(validator);
}

function isStringArray(value: unknown): value is string[] {
  return isArrayOf(isString)(value);
}

function isConnectionStatus(value: unknown): value is ConnectionStatus {
  return (
    isRecord(value) &&
    typeof value["connected"] === "boolean" &&
    isNullableString(value["server_url"]) &&
    isNullableString(value["username"]) &&
    isNullableString(value["server_version"])
  );
}

function isBackupSummary(value: unknown): value is BackupSummary {
  return (
    isRecord(value) &&
    typeof value["artists"] === "number" &&
    typeof value["albums"] === "number" &&
    typeof value["songs"] === "number" &&
    typeof value["playlists"] === "number" &&
    typeof value["queue_items"] === "number"
  );
}

function isConnectivitySettings(value: unknown): value is ConnectivitySettings {
  return (
    isRecord(value) && typeof value["manual_offline_enabled"] === "boolean"
  );
}

function isSyncSettings(value: unknown): value is SyncSettings {
  return (
    isRecord(value) &&
    typeof value["incremental_enabled"] === "boolean" &&
    typeof value["incremental_interval_minutes"] === "number" &&
    typeof value["full_reconcile_enabled"] === "boolean" &&
    typeof value["full_reconcile_interval_hours"] === "number"
  );
}

function isScanStatus(value: unknown): value is ScanStatus {
  return (
    isRecord(value) &&
    typeof value["scanning"] === "boolean" &&
    isNullableNumber(value["count"])
  );
}

function isArtist(value: unknown): value is Artist {
  return (
    isRecord(value) &&
    typeof value["id"] === "string" &&
    typeof value["name"] === "string" &&
    typeof value["album_count"] === "number" &&
    isNullableString(value["cover_art_id"]) &&
    typeof value["synced_at"] === "string"
  );
}

function isAlbum(value: unknown): value is Album {
  return (
    isRecord(value) &&
    typeof value["id"] === "string" &&
    typeof value["artist_id"] === "string" &&
    typeof value["name"] === "string" &&
    isNullableNumber(value["year"]) &&
    typeof value["song_count"] === "number" &&
    isNullableNumber(value["duration"]) &&
    isNullableString(value["cover_art_id"]) &&
    typeof value["synced_at"] === "string" &&
    isNullableString(value["artist_name"])
  );
}

function isAlbumListEntry(value: unknown): value is AlbumListEntry {
  return (
    isRecord(value) &&
    typeof value["id"] === "string" &&
    typeof value["name"] === "string" &&
    isNullableString(value["artist_id"]) &&
    isNullableString(value["artist_name"]) &&
    isNullableNumber(value["year"]) &&
    isNullableNumber(value["song_count"]) &&
    isNullableNumber(value["duration"]) &&
    isNullableString(value["cover_art_id"]) &&
    isNullableNumber(value["play_count"]) &&
    isNullableString(value["created"])
  );
}

function isSong(value: unknown): value is Song {
  return (
    isRecord(value) &&
    typeof value["id"] === "string" &&
    typeof value["album_id"] === "string" &&
    typeof value["artist_id"] === "string" &&
    typeof value["title"] === "string" &&
    isNullableNumber(value["track_number"]) &&
    typeof value["disc_number"] === "number" &&
    isNullableNumber(value["duration"]) &&
    isNullableNumber(value["bit_rate"]) &&
    isNullableNumber(value["size"]) &&
    isNullableString(value["suffix"]) &&
    isNullableString(value["content_type"]) &&
    isNullableString(value["path"]) &&
    isNullableNumber(value["year"]) &&
    isNullableString(value["genre"]) &&
    typeof value["synced_at"] === "string" &&
    isNullableString(value["artist"]) &&
    isNullableString(value["album"])
  );
}

function isPlaylist(value: unknown): value is Playlist {
  return (
    isRecord(value) &&
    typeof value["id"] === "string" &&
    typeof value["name"] === "string" &&
    typeof value["song_count"] === "number" &&
    typeof value["duration"] === "number" &&
    isNullableString(value["owner"]) &&
    isNullableString(value["cover_art_id"]) &&
    typeof value["created_at"] === "string" &&
    typeof value["changed_at"] === "string" &&
    typeof value["saved_offline"] === "boolean" &&
    isNullableString(value["offline_saved_at"])
  );
}

function isSearchResultSong(
  value: unknown
): value is SearchResults["songs"][number] {
  return (
    isRecord(value) &&
    typeof value["id"] === "string" &&
    typeof value["title"] === "string" &&
    isNullableString(value["artist"]) &&
    isNullableString(value["album"]) &&
    isNullableNumber(value["duration"])
  );
}

function isSearchResultAlbum(
  value: unknown
): value is SearchResults["albums"][number] {
  return (
    isRecord(value) &&
    typeof value["id"] === "string" &&
    typeof value["name"] === "string" &&
    isNullableString(value["artist"]) &&
    isNullableNumber(value["year"]) &&
    typeof value["song_count"] === "number"
  );
}

function isSearchResultArtist(
  value: unknown
): value is SearchResults["artists"][number] {
  return (
    isRecord(value) &&
    typeof value["id"] === "string" &&
    typeof value["name"] === "string" &&
    typeof value["album_count"] === "number"
  );
}

function isSearchResults(value: unknown): value is SearchResults {
  return (
    isRecord(value) &&
    isArrayOf(isSearchResultSong)(value["songs"]) &&
    isArrayOf(isSearchResultAlbum)(value["albums"]) &&
    isArrayOf(isSearchResultArtist)(value["artists"])
  );
}

function isRepeatMode(value: unknown): value is RepeatMode {
  return value === "Off" || value === "All" || value === "One";
}

function isQueueItem(value: unknown): value is QueueItem {
  return (
    isRecord(value) &&
    typeof value["song_id"] === "string" &&
    typeof value["title"] === "string" &&
    typeof value["artist"] === "string" &&
    typeof value["album"] === "string" &&
    typeof value["duration"] === "number"
  );
}

function isQueueState(value: unknown): value is QueueState {
  return (
    isRecord(value) &&
    Array.isArray(value["items"]) &&
    value["items"].every(isQueueItem) &&
    isNullableNumber(value["current_index"]) &&
    typeof value["shuffle"] === "boolean" &&
    isRepeatMode(value["repeat_mode"]) &&
    isNullableNumber(value["pending_navigation_index"]) &&
    (value["prepared_next_item"] === null ||
      isQueueItem(value["prepared_next_item"]))
  );
}

function isCacheStats(value: unknown): value is CacheStats {
  return (
    isRecord(value) &&
    typeof value["total_size"] === "number" &&
    typeof value["file_count"] === "number" &&
    typeof value["max_size"] === "number"
  );
}

function isDownloadStatus(value: unknown): value is DownloadStatus {
  return (
    isRecord(value) &&
    typeof value["song_id"] === "string" &&
    typeof value["cached"] === "boolean" &&
    isNullableString(value["path"]) &&
    typeof value["bytes"] === "number"
  );
}

function isSavedPlaylistOfflineResult(
  value: unknown
): value is SavedPlaylistOfflineResult {
  return (
    isRecord(value) &&
    typeof value["playlist_id"] === "string" &&
    typeof value["saved_offline"] === "boolean" &&
    typeof value["downloaded_count"] === "number" &&
    typeof value["removed_count"] === "number" &&
    typeof value["skipped_protected_count"] === "number"
  );
}

function isSavedPlaylistOfflineStatus(
  value: unknown
): value is SavedPlaylistOfflineStatus {
  return (
    isRecord(value) &&
    typeof value["running"] === "boolean" &&
    isNullableString(value["last_error"])
  );
}

function isFileStateSnapshot(value: unknown): value is FileStateSnapshot {
  return (
    isRecord(value) &&
    typeof value["seq"] === "number" &&
    isStringArray(value["downloaded_song_ids"]) &&
    isStringArray(value["downloading_song_ids"])
  );
}

function isPlaybackStateSnapshot(
  value: unknown
): value is PlaybackStateSnapshot {
  return (
    isRecord(value) &&
    isNullableString(value["current_song_id"]) &&
    typeof value["position_seconds"] === "number" &&
    typeof value["duration_seconds"] === "number" &&
    typeof value["was_playing"] === "boolean" &&
    typeof value["app_volume"] === "number" &&
    typeof value["updated_at"] === "string"
  );
}

function isPlaybackSnapshotSong(
  value: unknown
): value is NonNullable<PlaybackSnapshot["song"]> {
  return (
    isRecord(value) &&
    typeof value["id"] === "string" &&
    typeof value["title"] === "string" &&
    typeof value["artist"] === "string" &&
    typeof value["album"] === "string" &&
    typeof value["duration_seconds"] === "number" &&
    isNullableString(value["artwork_uri"])
  );
}

export function isPlaybackSnapshot(value: unknown): value is PlaybackSnapshot {
  return (
    isRecord(value) &&
    typeof value["seq"] === "number" &&
    ["playing", "paused", "stopped", "stalled"].includes(
      typeof value["state"] === "string" ? value["state"] : ""
    ) &&
    typeof value["is_playing"] === "boolean" &&
    typeof value["audio_loaded"] === "boolean" &&
    ["closed", "ready", "failed", "unavailable"].includes(
      typeof value["output_state"] === "string" ? value["output_state"] : ""
    ) &&
    (value["song"] === null || isPlaybackSnapshotSong(value["song"])) &&
    typeof value["position_seconds"] === "number" &&
    typeof value["duration_seconds"] === "number" &&
    typeof value["volume"] === "number" &&
    isQueueState(value["queue"]) &&
    isNullableNumber(value["queue_index"]) &&
    typeof value["queue_length"] === "number" &&
    typeof value["can_play"] === "boolean" &&
    typeof value["can_next"] === "boolean" &&
    typeof value["can_previous"] === "boolean" &&
    typeof value["can_seek"] === "boolean"
  );
}

function isSyncJobStatus(
  value: unknown
): value is LibrarySyncStatus["incremental"] {
  return (
    isRecord(value) &&
    typeof value["enabled"] === "boolean" &&
    typeof value["interval_minutes"] === "number" &&
    typeof value["running"] === "boolean" &&
    isNullableString(value["last_attempt_at"]) &&
    isNullableString(value["last_success_at"]) &&
    isNullableString(value["last_error"]) &&
    isNullableString(value["next_run_at"])
  );
}

export function isLibrarySyncStatus(
  value: unknown
): value is LibrarySyncStatus {
  return (
    isRecord(value) &&
    isNullableString(value["active_job"]) &&
    isSyncJobStatus(value["full"]) &&
    isSyncJobStatus(value["incremental"]) &&
    isSyncJobStatus(value["full_reconcile"])
  );
}

function isLastfmStatus(value: unknown): value is LastfmStatus {
  return (
    isRecord(value) &&
    typeof value["available"] === "boolean" &&
    typeof value["authenticated"] === "boolean" &&
    typeof value["enabled"] === "boolean" &&
    isNullableString(value["username"]) &&
    typeof value["pending_auth"] === "boolean" &&
    typeof value["queue_count"] === "number" &&
    isNullableString(value["last_error"])
  );
}

function isLastfmAuthStart(value: unknown): value is LastfmAuthStart {
  return isRecord(value) && typeof value["auth_url"] === "string";
}

function isLastfmQueueItem(value: unknown): value is LastfmQueueItem {
  return (
    isRecord(value) &&
    typeof value["id"] === "number" &&
    typeof value["song_id"] === "string" &&
    typeof value["title"] === "string" &&
    typeof value["artist"] === "string" &&
    isNullableString(value["album"]) &&
    isNullableNumber(value["duration"]) &&
    typeof value["played_at"] === "number" &&
    typeof value["attempts"] === "number" &&
    typeof value["next_retry_at"] === "number" &&
    isNullableString(value["last_error"]) &&
    typeof value["created_at"] === "string" &&
    typeof value["updated_at"] === "string"
  );
}

function isPlaybackState(value: unknown): boolean {
  return (
    value === "playing" ||
    value === "paused" ||
    value === "stopped" ||
    value === "stalled"
  );
}

function isAudioPlaybackStatus(value: unknown): value is AudioPlaybackStatus {
  return (
    isRecord(value) &&
    isPlaybackState(value["state"]) &&
    typeof value["is_playing"] === "boolean" &&
    isNullableString(value["current_song_id"]) &&
    typeof value["position"] === "number" &&
    typeof value["duration"] === "number" &&
    typeof value["volume"] === "number"
  );
}

function isNormalizationMode(
  value: unknown
): value is AudioProcessingSettings["normalization_mode"] {
  return value === "track" || value === "album";
}

function isDynamicsPreset(
  value: unknown
): value is AudioProcessingSettings["dynamics_preset"] {
  return value === "light" || value === "medium" || value === "heavy";
}

function isBinauralPreset(
  value: unknown
): value is AudioProcessingSettings["binaural_preset"] {
  return value === "light" || value === "medium" || value === "strong";
}

function isAudioProcessingSettings(
  value: unknown
): value is AudioProcessingSettings {
  return (
    isRecord(value) &&
    typeof value["normalization_enabled"] === "boolean" &&
    isNormalizationMode(value["normalization_mode"]) &&
    typeof value["target_lufs"] === "number" &&
    typeof value["preamp_db"] === "number" &&
    typeof value["prevent_clipping"] === "boolean" &&
    typeof value["dynamics_enabled"] === "boolean" &&
    isDynamicsPreset(value["dynamics_preset"]) &&
    typeof value["binaural_enabled"] === "boolean" &&
    isBinauralPreset(value["binaural_preset"]) &&
    typeof value["equalizer_enabled"] === "boolean" &&
    isArrayOf(isNumber)(value["equalizer_bands_db"]) &&
    typeof value["gapless_enabled"] === "boolean" &&
    typeof value["crossfade_enabled"] === "boolean" &&
    typeof value["crossfade_duration_ms"] === "number" &&
    typeof value["prefetch_count"] === "number"
  );
}

async function invokeJson<T>(
  name: string,
  isPayload: PayloadValidator<T>,
  payload: unknown = null
): Promise<T> {
  await ensureInitialized();

  if (NativeStereodromeCore.call === undefined) {
    throw new Error(unavailable);
  }
  return parseEnvelope<T>(
    await NativeStereodromeCore.call(name, JSON.stringify(payload)),
    isPayload
  );
}

async function dispatchRuntimeCommand(command: Record<string, unknown>) {
  await ensureInitialized();
  if (NativeStereodromeCore.dispatch === undefined) {
    throw new Error(unavailable);
  }
  const commandId = nextRuntimeCommandId++;
  const parsed: unknown = JSON.parse(
    await NativeStereodromeCore.dispatch(
      JSON.stringify({
        protocol_version: 1,
        command_id: commandId,
        command,
      })
    )
  );
  if (
    !isRecord(parsed) ||
    parsed["protocol_version"] !== 1 ||
    parsed["command_id"] !== commandId ||
    (parsed["status"] !== "succeeded" && parsed["status"] !== "failed")
  ) {
    throw new Error("Native core returned an invalid runtime result");
  }
  if (parsed["status"] === "failed") {
    const error = parsed["error"];
    throw new Error(
      isRecord(error) && typeof error["message"] === "string"
        ? error["message"]
        : "Runtime command failed"
    );
  }
  return parsed["value"];
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
  return invokeJson(name, isQueueState, payload);
}

async function ensureInitialized(): Promise<boolean> {
  if (initializePromise !== null) {
    return initializePromise;
  }

  initializePromise = (async () => {
    if (NativeStereodromeCore.initialize === undefined) {
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
  getConnectionStatus(): Promise<ConnectionStatus> {
    return invokeJson("getConnectionStatus", isConnectionStatus);
  },
  connectServer(params: {
    url: string;
    username: string;
    password: string;
  }): Promise<ConnectionStatus> {
    return invokeJson("connectServer", isConnectionStatus, params);
  },
  updateServerSettings(params: {
    url?: string;
    username?: string;
  }): Promise<ConnectionStatus> {
    return invokeJson("updateServerSettings", isConnectionStatus, params);
  },
  restoreSession(): Promise<ConnectionStatus> {
    return invokeJson("restoreSession", isConnectionStatus);
  },
  exportPortableBackup(path: string): Promise<BackupSummary> {
    return invokeJson("exportPortableBackup", isBackupSummary, path);
  },
  importPortableBackup(path: string): Promise<BackupSummary> {
    return invokeJson("importPortableBackup", isBackupSummary, path);
  },
  disconnectServer(): Promise<void> {
    return invokeJson("disconnectServer", isNull).then(() => undefined);
  },
  syncLibrary(): Promise<void> {
    return invokeJson("syncLibrary", isNull).then(() => undefined);
  },
  syncLibraryIncremental(): Promise<void> {
    return invokeJson("syncLibraryIncremental", isNull).then(() => undefined);
  },
  getSyncSettings(): Promise<SyncSettings> {
    return invokeJson("getSyncSettings", isSyncSettings);
  },
  setSyncSettings(settings: SyncSettings): Promise<SyncSettings> {
    return invokeJson("setSyncSettings", isSyncSettings, settings);
  },
  getConnectivitySettings(): Promise<ConnectivitySettings> {
    return invokeJson("getConnectivitySettings", isConnectivitySettings);
  },
  setConnectivitySettings(
    settings: ConnectivitySettings
  ): Promise<ConnectivitySettings> {
    return invokeJson(
      "setConnectivitySettings",
      isConnectivitySettings,
      settings
    );
  },
  reportNetwork(available: boolean): Promise<void> {
    return dispatchRuntimeCommand({ type: "report-network", available }).then(
      () => undefined
    );
  },
  reportLifecycle(lifecycle: "foreground" | "background"): Promise<void> {
    return dispatchRuntimeCommand({
      type: "report-lifecycle",
      lifecycle,
    }).then(() => undefined);
  },
  runDueLibrarySync(): Promise<string | null> {
    return invokeJson("runDueLibrarySync", isNullable(isString));
  },
  getScanStatus(): Promise<ScanStatus> {
    return invokeJson("getScanStatus", isScanStatus);
  },
  startScan(): Promise<ScanStatus> {
    return invokeJson("startScan", isScanStatus);
  },
  getLibrarySyncStatus(): Promise<LibrarySyncStatus> {
    return invokeJson("getLibrarySyncStatus", isLibrarySyncStatus);
  },
  getArtists(): Promise<Artist[]> {
    return invokeJson("getArtists", isArrayOf(isArtist));
  },
  getAlbums(artistId?: string): Promise<Album[]> {
    return invokeJson("getAlbums", isArrayOf(isAlbum), artistId ?? null);
  },
  getAlbumList(
    listType: string,
    size = 50,
    offset = 0
  ): Promise<AlbumListEntry[]> {
    return invokeJson("getAlbumList", isArrayOf(isAlbumListEntry), {
      list_type: listType,
      size,
      offset,
    });
  },
  getSongs(albumId?: string, artistId?: string): Promise<Song[]> {
    return invokeJson("getSongs", isArrayOf(isSong), {
      first: albumId ?? null,
      second: artistId ?? null,
    });
  },
  getPlaylists(): Promise<Playlist[]> {
    return invokeJson("getPlaylists", isArrayOf(isPlaylist));
  },
  getPlaylistSongs(id: string): Promise<Song[]> {
    return invokeJson("getPlaylistSongs", isArrayOf(isSong), id);
  },
  createPlaylist(name: string, songIds: string[] = []): Promise<Playlist> {
    return invokeJson("createPlaylist", isPlaylist, {
      name,
      song_ids: songIds,
    });
  },
  renamePlaylist(playlistId: string, name: string): Promise<void> {
    return invokeJson("renamePlaylist", isNull, {
      playlist_id: playlistId,
      name,
    }).then(() => undefined);
  },
  deletePlaylist(playlistId: string): Promise<void> {
    return invokeJson("deletePlaylist", isNull, playlistId).then(
      () => undefined
    );
  },
  addSongsToPlaylist(playlistId: string, songIds: string[]): Promise<void> {
    return invokeJson("addSongsToPlaylist", isNull, {
      playlist_id: playlistId,
      song_ids: songIds,
    }).then(() => undefined);
  },
  removeSongsFromPlaylist(
    playlistId: string,
    songIndexes: number[]
  ): Promise<void> {
    return invokeJson("removeSongsFromPlaylist", isNull, {
      playlist_id: playlistId,
      song_indexes: songIndexes,
    }).then(() => undefined);
  },
  searchLibrary(query: string, limit = 25): Promise<SearchResults> {
    return invokeJson("searchLibrary", isSearchResults, { query, limit });
  },
  getCoverArtUri(coverArtId: string, size = 512): Promise<string> {
    return invokeJson("getCoverArtUri", isString, { id: coverArtId, size });
  },
  getSongCoverArtUri(songId: string, size = 512): Promise<string | null> {
    return invokeJson("getSongCoverArtUri", isNullable(isString), {
      id: songId,
      size,
    });
  },
  getStreamUri(songId: string): Promise<string> {
    return invokeJson("getStreamUri", isString, songId);
  },
  getAudioCacheStats(): Promise<CacheStats> {
    return invokeJson("getAudioCacheStats", isCacheStats);
  },
  getOfflineSongIds(): Promise<string[]> {
    return invokeJson("getOfflineSongIds", isStringArray);
  },
  getFileStateSnapshot(): Promise<FileStateSnapshot> {
    return invokeJson("getFileStateSnapshot", isFileStateSnapshot);
  },
  setMaxCacheSize(maxSize: number): Promise<CacheStats> {
    return invokeJson("setMaxCacheSize", isCacheStats, maxSize);
  },
  clearAudioCache(): Promise<CacheStats> {
    return invokeJson("clearAudioCache", isCacheStats);
  },
  isSongCached(songId: string): Promise<DownloadStatus> {
    return invokeJson("isSongCached", isDownloadStatus, songId);
  },
  downloadSong(songId: string): Promise<DownloadStatus> {
    return invokeJson("downloadSong", isDownloadStatus, songId);
  },
  removeCachedSong(songId: string): Promise<DownloadStatus> {
    return invokeJson("removeCachedSong", isDownloadStatus, songId);
  },
  downloadAlbum(albumId: string): Promise<DownloadStatus[]> {
    return invokeJson("downloadAlbum", isArrayOf(isDownloadStatus), albumId);
  },
  downloadPlaylist(playlistId: string): Promise<DownloadStatus[]> {
    return invokeJson(
      "downloadPlaylist",
      isArrayOf(isDownloadStatus),
      playlistId
    );
  },
  setPlaylistSavedOffline(
    playlistId: string,
    savedOffline: boolean
  ): Promise<SavedPlaylistOfflineResult> {
    return invokeJson("setPlaylistSavedOffline", isSavedPlaylistOfflineResult, {
      playlist_id: playlistId,
      saved_offline: savedOffline,
    });
  },
  reconcileSavedPlaylistsOffline(): Promise<SavedPlaylistOfflineResult[]> {
    return invokeJson(
      "reconcileSavedPlaylistsOffline",
      isArrayOf(isSavedPlaylistOfflineResult)
    );
  },
  startSavedPlaylistsOfflineReconcile(): Promise<void> {
    return invokeJson("startSavedPlaylistsOfflineReconcile", isNull).then(
      () => undefined
    );
  },
  getSavedPlaylistsOfflineReconcileStatus(): Promise<SavedPlaylistOfflineStatus> {
    return invokeJson(
      "getSavedPlaylistsOfflineReconcileStatus",
      isSavedPlaylistOfflineStatus
    );
  },
  prefetchNext(reserveFirst = false): Promise<void> {
    return invokeJson("prefetchNext", isNull, {
      reserve_first: reserveFirst,
    }).then(() => undefined);
  },
  getPlaybackState(): Promise<PlaybackStateSnapshot> {
    return invokeJson("getPlaybackState", isPlaybackStateSnapshot);
  },
  getPlaybackSnapshot(): Promise<PlaybackSnapshot> {
    return invokeJson("getPlaybackSnapshot", isPlaybackSnapshot);
  },
  savePlaybackPosition(
    progress: PlaybackProgress
  ): Promise<PlaybackStateSnapshot> {
    return invokeJson(
      "savePlaybackPosition",
      isPlaybackStateSnapshot,
      progress
    );
  },
  getLastfmStatus(): Promise<LastfmStatus> {
    return invokeJson("getLastfmStatus", isLastfmStatus);
  },
  beginLastfmAuth(): Promise<LastfmAuthStart> {
    return invokeJson("beginLastfmAuth", isLastfmAuthStart);
  },
  completeLastfmAuth(): Promise<LastfmStatus> {
    return invokeJson("completeLastfmAuth", isLastfmStatus);
  },
  disconnectLastfm(): Promise<LastfmStatus> {
    return invokeJson("disconnectLastfm", isLastfmStatus);
  },
  getLastfmQueue(): Promise<LastfmQueueItem[]> {
    return invokeJson("getLastfmQueue", isArrayOf(isLastfmQueueItem));
  },
  retryLastfmQueue(): Promise<number> {
    return invokeJson("retryLastfmQueue", isNumber);
  },
  audioPlayCurrent(): Promise<AudioPlaybackStatus> {
    return invokeJson("audioPlayCurrent", isAudioPlaybackStatus);
  },
  audioPlayQueueItem(index: number): Promise<AudioPlaybackStatus> {
    return invokeJson("audioPlayQueueItem", isAudioPlaybackStatus, index);
  },
  audioPlayNext(force = true): Promise<AudioPlaybackStatus> {
    return invokeJson("audioPlayNext", isAudioPlaybackStatus, force);
  },
  audioPlayPrevious(): Promise<AudioPlaybackStatus> {
    return invokeJson("audioPlayPrevious", isAudioPlaybackStatus);
  },
  audioApplySettings(): Promise<AudioPlaybackStatus> {
    return invokeJson("audioApplySettings", isAudioPlaybackStatus);
  },
  audioPrepareNextTransition(): Promise<void> {
    return invokeJson("audioPrepareNextTransition", isNull).then(
      () => undefined
    );
  },
  audioPause(): Promise<void> {
    return invokeJson("audioPause", isNull).then(() => undefined);
  },
  togglePlayback(): Promise<void> {
    return dispatchRuntimeCommand({ type: "toggle-playback" }).then(
      () => undefined
    );
  },
  audioResume(): Promise<void> {
    return invokeJson("audioResume", isNull).then(() => undefined);
  },
  audioRebuildOutput(): Promise<void> {
    return invokeJson("audioRebuildOutput", isNull).then(() => undefined);
  },
  audioStop(): Promise<void> {
    return invokeJson("audioStop", isNull).then(() => undefined);
  },
  audioSeek(positionSeconds: number): Promise<void> {
    return invokeJson("audioSeek", isNull, positionSeconds).then(
      () => undefined
    );
  },
  seekBy(seconds: number): Promise<void> {
    return dispatchRuntimeCommand({ type: "seek-by", seconds }).then(
      () => undefined
    );
  },
  audioSetVolume(volume: number): Promise<void> {
    return invokeJson("audioSetVolume", isNull, volume).then(() => undefined);
  },
  getAudioProcessingSettings(): Promise<AudioProcessingSettings> {
    return invokeJson("getAudioProcessingSettings", isAudioProcessingSettings);
  },
  setAudioProcessingSettings(
    settings: AudioProcessingSettings
  ): Promise<AudioProcessingSettings> {
    return invokeJson(
      "setAudioProcessingSettings",
      isAudioProcessingSettings,
      settings
    );
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
    await invokeJson("playQueueItem", isNullable(isQueueItem), index);
    return (await this.getPlaybackSnapshot()).queue;
  },
  async playNext(force = true): Promise<QueueState> {
    await invokeJson("playNext", isNullable(isQueueItem), force);
    return (await this.getPlaybackSnapshot()).queue;
  },
  async playPrevious(): Promise<QueueState> {
    await invokeJson("playPrevious", isNullable(isQueueItem));
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
