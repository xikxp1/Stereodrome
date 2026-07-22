import { useEffect, useMemo, useState, useSyncExternalStore } from "react";

import { coreActions, coreStore } from "@/core/store";
import type {
  ConnectivityState,
  PlaybackProjection,
} from "@/core/protocol.generated";
import type { ConnectionStatus, PlayableSong, QueueItem } from "@/types/music";

const disconnected: ConnectionStatus = {
  connected: false,
  server_url: null,
  username: null,
  server_version: null,
};

function useCoreState() {
  return useSyncExternalStore(
    coreStore.subscribe,
    coreStore.getState,
    coreStore.getState
  );
}

function playableFromQueueItem(item: QueueItem): PlayableSong {
  return {
    id: item.song_id,
    title: item.title,
    artist: item.artist,
    album: item.album,
    duration: item.duration,
  };
}

function connectionStatus(connectivity: ConnectivityState): ConnectionStatus {
  switch (connectivity.status) {
    case "unconfigured":
      return disconnected;
    case "offline-manual":
      return {
        connected: false,
        server_url: connectivity.server_url,
        username: connectivity.username,
        server_version: null,
      };
    case "disconnected":
      return {
        connected: false,
        server_url: connectivity.server_url,
        username: connectivity.username,
        server_version: null,
      };
    case "online":
      return {
        connected: true,
        server_url: connectivity.server_url,
        username: connectivity.username,
        server_version: connectivity.server_version,
      };
  }
}

function currentSong(playback: PlaybackProjection | null): PlayableSong | null {
  return playback?.song === null || playback?.song === undefined
    ? null
    : {
        id: playback.song.id,
        title: playback.song.title,
        artist: playback.song.artist,
        album: playback.song.album,
        duration: playback.song.duration_seconds,
      };
}

function nextSong(playback: PlaybackProjection | null): PlayableSong | null {
  if (playback === null || playback.queue.items.length === 0) {
    return null;
  }
  if (playback.queue.prepared_next_item !== null) {
    return playableFromQueueItem(playback.queue.prepared_next_item);
  }
  const index = playback.queue.current_index;
  if (playback.queue.repeat_mode === "One" && index !== null) {
    const item = playback.queue.items[index];
    return item === undefined ? null : playableFromQueueItem(item);
  }
  if (index === null) {
    const pending = playback.queue.pending_navigation_index ?? 0;
    const item =
      playback.queue.items[Math.min(pending, playback.queue.items.length - 1)];
    return item === undefined ? null : playableFromQueueItem(item);
  }
  const item = playback.queue.items[index + 1];
  if (item !== undefined) {
    return playableFromQueueItem(item);
  }
  const first = playback.queue.items[0];
  return playback.queue.repeat_mode === "All" && first !== undefined
    ? playableFromQueueItem(first)
    : null;
}

function useInterpolatedPosition(playback: PlaybackProjection | null): number {
  const authoritative = playback?.position_seconds ?? 0;
  const [position, setPosition] = useState(authoritative);

  useEffect(() => {
    setPosition(authoritative);
    if (playback?.is_playing !== true) {
      return undefined;
    }
    const interval = setInterval(() => {
      setPosition((current) =>
        playback.duration_seconds > 0
          ? Math.min(playback.duration_seconds, current + 1)
          : current + 1
      );
    }, 1000);
    return () => {
      clearInterval(interval);
    };
  }, [authoritative, playback?.duration_seconds, playback?.is_playing]);

  return position;
}

const playbackActions = {
  clearQueue: coreActions.clearPlayback,
  next: () => coreActions.navigate({ type: "next", force: true }),
  playQueueIndex: (index: number) =>
    coreActions.navigate({ type: "index", index }),
  playSong: coreActions.playSong,
  previous: () => coreActions.navigate({ type: "previous" }),
  removeQueueIndex: coreActions.removeQueueIndex,
  rerollNext: coreActions.rerollNext,
  seekBy: coreActions.seekBy,
  toggle: coreActions.togglePlayback,
  toggleRepeat: coreActions.cycleRepeatMode,
  toggleShuffle: coreActions.toggleShuffle,
};

export function usePlaybackActions() {
  return playbackActions;
}

export function usePlaybackMetadata() {
  const { error, snapshot } = useCoreState();
  const playback = snapshot?.playback ?? null;
  return useMemo(
    () => ({
      currentSong: currentSong(playback),
      error,
      isPlaying: playback?.is_playing ?? false,
      nextSong: nextSong(playback),
      queue: playback?.queue.items.map(playableFromQueueItem) ?? [],
      repeatMode: playback?.queue.repeat_mode ?? ("Off" as const),
      repeatEnabled: (playback?.queue.repeat_mode ?? "Off") !== "Off",
      shuffleEnabled: playback?.queue.shuffle ?? false,
    }),
    [error, playback]
  );
}

export function usePlayback() {
  const metadata = usePlaybackMetadata();
  const playback = useCoreState().snapshot?.playback ?? null;
  const position = useInterpolatedPosition(playback);
  return useMemo(
    () => ({
      ...metadata,
      ...playbackActions,
      duration: playback?.duration_seconds ?? 0,
      position,
    }),
    [metadata, playback?.duration_seconds, position]
  );
}

export function useStereodrome() {
  const { error, ready, snapshot } = useCoreState();
  const connectivity = snapshot?.connectivity ?? null;
  return useMemo(() => {
    const status =
      connectivity === null ? disconnected : connectionStatus(connectivity);
    const manualOfflineEnabled = connectivity?.status === "offline-manual";
    const hasConfiguredServer = status.server_url !== null;
    return {
      ready,
      status,
      hasConfiguredServer,
      offlineMode:
        manualOfflineEnabled || (hasConfiguredServer && !status.connected),
      manualOfflineEnabled,
      error,
      refreshStatus: coreActions.reconcile,
      connect: coreActions.connect,
      updateServerSettings: coreActions.updateServerSettings,
      setManualOfflineEnabled: coreActions.setManualOfflineEnabled,
      sync: () => coreActions.sync("full"),
      syncIncremental: () => coreActions.sync("incremental"),
      disconnect: coreActions.disconnect,
    };
  }, [connectivity, error, ready]);
}

export function useFileState() {
  const downloads = useCoreState().snapshot?.downloads ?? null;
  return useMemo(
    () => ({
      offlineSongIds: new Set(downloads?.offline_song_ids ?? []),
      downloadingSongIds: new Set(downloads?.downloading_song_ids ?? []),
    }),
    [downloads]
  );
}

export function useSyncStatus() {
  return useCoreState().snapshot?.sync ?? null;
}
