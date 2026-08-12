import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { AppState } from "react-native";

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

function resolveNextSong(
  playback: PlaybackProjection | null,
  force: boolean
): PlayableSong | null {
  if (playback === null || playback.queue.items.length === 0) {
    return null;
  }

  const queue = playback.queue;
  const index = queue.current_index;
  const pending = queue.pending_navigation_index;
  const repeat = queue.repeat_mode;
  const first = queue.items[0];

  // Current track was removed: resume wherever the cursor now points.
  if (index === null && pending !== null) {
    if (pending < queue.items.length) {
      const item = queue.items[pending];
      return item === undefined ? null : playableFromQueueItem(item);
    }
    // The cursor sits past the last item, so only a wrap can continue.
    const wraps = repeat === "All" || (repeat === "One" && force);
    return wraps && first !== undefined ? playableFromQueueItem(first) : null;
  }

  if (queue.prepared_next_item !== null) {
    return playableFromQueueItem(queue.prepared_next_item);
  }

  // Auto-advance under repeat one replays the current track, so an idle queue has
  // nothing to advance to.
  if (repeat === "One" && !force) {
    if (index === null) {
      return null;
    }
    const item = queue.items[index];
    return item === undefined ? null : playableFromQueueItem(item);
  }

  if (index === null) {
    return first === undefined ? null : playableFromQueueItem(first);
  }

  const item = queue.items[index + 1];
  if (item !== undefined) {
    return playableFromQueueItem(item);
  }

  // At the end - wrap unless repeat is off.
  return repeat !== "Off" && first !== undefined
    ? playableFromQueueItem(first)
    : null;
}

// The song the Next control will play. Skipping is forced navigation, so under repeat
// one it advances rather than replaying the current track.
function nextSong(playback: PlaybackProjection | null): PlayableSong | null {
  return resolveNextSong(playback, true);
}

// The song that will play once the current one ends on its own.
function upcomingSong(
  playback: PlaybackProjection | null
): PlayableSong | null {
  return resolveNextSong(playback, false);
}

function useInterpolatedPosition(playback: PlaybackProjection | null): number {
  const authoritative = playback?.position_seconds ?? 0;
  const [position, setPosition] = useState(authoritative);

  useEffect(() => {
    setPosition(authoritative);
    if (playback?.is_playing !== true) {
      return undefined;
    }
    // Ticks pause while the app is backgrounded; the foreground reconcile in
    // App.tsx refreshes the authoritative position when the app returns.
    let interval: ReturnType<typeof setInterval> | null = null;
    const start = () => {
      interval ??= setInterval(() => {
        setPosition((current) =>
          playback.duration_seconds > 0
            ? Math.min(playback.duration_seconds, current + 1)
            : current + 1
        );
      }, 1000);
    };
    const stop = () => {
      if (interval !== null) {
        clearInterval(interval);
        interval = null;
      }
    };
    if (AppState.currentState === "active") {
      start();
    }
    const subscription = AppState.addEventListener("change", (nextState) => {
      if (nextState === "active") {
        start();
      } else {
        stop();
      }
    });
    return () => {
      stop();
      subscription.remove();
    };
  }, [authoritative, playback?.duration_seconds, playback?.is_playing]);

  return position;
}

/**
 * The 1 Hz interpolated playback position. This is the only hook that
 * re-renders every second, so consume it in leaf components rather than in
 * screen- or shell-level ones.
 */
export function usePlaybackPosition(): number {
  const playback = useCoreState().snapshot?.playback ?? null;
  return useInterpolatedPosition(playback);
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
      upcomingSong: upcomingSong(playback),
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
  return useMemo(
    () => ({
      ...metadata,
      ...playbackActions,
      duration: playback?.duration_seconds ?? 0,
    }),
    [metadata, playback?.duration_seconds]
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
