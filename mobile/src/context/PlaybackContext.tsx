import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { AppState, type AppStateStatus } from "react-native";

import { stereodromeCore } from "@/services/stereodromeCore";
import type {
  PlaybackSnapshot,
  PlayableSong,
  QueueItem,
  QueueState,
} from "@/types/music";

type PlaybackContextValue = {
  currentSong: PlayableSong | null;
  duration: number;
  error: string | null;
  isPlaying: boolean;
  nextSong: PlayableSong | null;
  position: number;
  repeatMode: QueueState["repeat_mode"];
  repeatEnabled: boolean;
  shuffleEnabled: boolean;
  queue: PlayableSong[];
  playSong(song: PlayableSong, queue?: PlayableSong[]): Promise<void>;
  toggle(): Promise<void>;
  toggleRepeat(): Promise<void>;
  toggleShuffle(): Promise<void>;
  rerollNext(): Promise<void>;
  seekBy(seconds: number): Promise<void>;
  next(): Promise<void>;
  previous(): Promise<void>;
  playQueueIndex(index: number): Promise<void>;
  removeQueueIndex(index: number): Promise<void>;
  clearQueue(): Promise<void>;
};

type PlaybackActionsContextValue = Pick<
  PlaybackContextValue,
  | "playSong"
  | "toggle"
  | "toggleRepeat"
  | "toggleShuffle"
  | "rerollNext"
  | "seekBy"
  | "next"
  | "previous"
  | "playQueueIndex"
  | "removeQueueIndex"
  | "clearQueue"
>;
type PlaybackMetadataContextValue = Pick<
  PlaybackContextValue,
  | "currentSong"
  | "error"
  | "isPlaying"
  | "nextSong"
  | "queue"
  | "repeatMode"
  | "repeatEnabled"
  | "shuffleEnabled"
>;

const PlaybackContext = createContext<PlaybackContextValue | null>(null);
const PlaybackActionsContext =
  createContext<PlaybackActionsContextValue | null>(null);
const PlaybackMetadataContext =
  createContext<PlaybackMetadataContextValue | null>(null);

async function ensurePlayerReady() {
  await stereodromeCore.initialize();
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
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

function playableSongsEqual(
  left: PlayableSong | null,
  right: PlayableSong | null
): boolean {
  return (
    left === right ||
    (left !== null &&
      right !== null &&
      left.id === right.id &&
      left.title === right.title &&
      left.artist === right.artist &&
      left.album === right.album &&
      left.duration === right.duration)
  );
}

function canonicalizeQueue(
  current: PlayableSong[],
  items: QueueItem[]
): PlayableSong[] {
  let changed = current.length !== items.length;
  const next = items.map((item, index) => {
    const song = playableFromQueueItem(item);
    const previous = current[index] ?? null;
    if (previous !== null && playableSongsEqual(previous, song)) {
      return previous;
    }
    changed = true;
    return song;
  });
  return changed ? next : current;
}

type NextSongInput = {
  currentIndex: number | null;
  pendingNavigationIndex: number | null;
  preparedNextSong: PlayableSong | null;
  queue: PlayableSong[];
  repeatMode: QueueState["repeat_mode"];
};

function getNextSong({
  currentIndex,
  pendingNavigationIndex,
  preparedNextSong,
  queue,
  repeatMode,
}: NextSongInput): PlayableSong | null {
  if (queue.length === 0) {
    return null;
  }

  if (preparedNextSong) {
    return preparedNextSong;
  }

  if (repeatMode === "One" && currentIndex !== null) {
    return queue[currentIndex] ?? null;
  }

  if (currentIndex === null) {
    if (pendingNavigationIndex === null) {
      return queue[0] ?? null;
    }

    return queue[Math.min(pendingNavigationIndex, queue.length - 1)] ?? null;
  }

  if (currentIndex + 1 < queue.length) {
    return queue[currentIndex + 1] ?? null;
  }

  if (repeatMode === "All") {
    return queue[0] ?? null;
  }

  return null;
}

export function PlaybackProvider({ children }: { children: React.ReactNode }) {
  const [error, setError] = useState<string | null>(null);
  const [currentSong, setCurrentSong] = useState<PlayableSong | null>(null);
  const [currentIndex, setCurrentIndex] = useState<number | null>(null);
  const [pendingNavigationIndex, setPendingNavigationIndex] = useState<
    number | null
  >(null);
  const [preparedNextSong, setPreparedNextSong] = useState<PlayableSong | null>(
    null
  );
  const [queue, setQueue] = useState<PlayableSong[]>([]);
  const [repeatMode, setRepeatMode] =
    useState<QueueState["repeat_mode"]>("Off");
  const [shuffleEnabled, setShuffleEnabled] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const currentSongRef = useRef<PlayableSong | null>(null);
  const queueRef = useRef<PlayableSong[]>([]);
  const isPlayingRef = useRef(false);
  const positionRef = useRef(0);
  const durationRef = useRef(0);
  const lastSnapshotSeqRef = useRef(0);
  const actionLocksRef = useRef(new Set<string>());

  const runPlaybackAction = useCallback(
    async (key: string, action: () => Promise<void>) => {
      if (actionLocksRef.current.has(key)) {
        return;
      }

      actionLocksRef.current.add(key);
      try {
        await action();
      } catch (playbackError) {
        setError(errorMessage(playbackError));
      } finally {
        actionLocksRef.current.delete(key);
      }
    },
    []
  );

  const applyQueueState = useCallback((state: QueueState) => {
    const songs = canonicalizeQueue(queueRef.current, state.items);
    queueRef.current = songs;
    const preparedSong = state.prepared_next_item
      ? playableFromQueueItem(state.prepared_next_item)
      : null;
    const index = state.current_index;
    const pendingIndex = state.pending_navigation_index;

    setQueue(songs);
    setCurrentIndex(index);
    setPendingNavigationIndex(pendingIndex);
    setPreparedNextSong((current) =>
      playableSongsEqual(current, preparedSong) ? current : preparedSong
    );
    setRepeatMode(state.repeat_mode);
    setShuffleEnabled(state.shuffle);
  }, []);

  const applyPlaybackSnapshot = useCallback(
    (snapshot: PlaybackSnapshot) => {
      if (snapshot.seq <= lastSnapshotSeqRef.current) {
        return;
      }
      lastSnapshotSeqRef.current = snapshot.seq;

      applyQueueState(snapshot.queue);

      const nextCurrent =
        snapshot.song === null
          ? null
          : {
              id: snapshot.song.id,
              title: snapshot.song.title,
              artist: snapshot.song.artist,
              album: snapshot.song.album,
              duration: snapshot.song.duration_seconds,
            };
      const current = playableSongsEqual(currentSongRef.current, nextCurrent)
        ? currentSongRef.current
        : nextCurrent;

      isPlayingRef.current = snapshot.is_playing;
      positionRef.current = snapshot.position_seconds;
      durationRef.current = snapshot.duration_seconds;
      currentSongRef.current = current;

      setIsPlaying(snapshot.is_playing);
      setPosition(snapshot.position_seconds);
      setDuration(snapshot.duration_seconds);
      setCurrentSong(current);
    },
    [applyQueueState]
  );

  const reconcilePlaybackSnapshot = useCallback(async () => {
    applyPlaybackSnapshot(await stereodromeCore.getPlaybackSnapshot());
  }, [applyPlaybackSnapshot]);

  const finalizePlaybackStart = useCallback(async () => {
    await reconcilePlaybackSnapshot();
  }, [reconcilePlaybackSnapshot]);

  const persistPlaybackPosition = useCallback(async (playing: boolean) => {
    const song = currentSongRef.current;
    if (!song) {
      return;
    }

    await stereodromeCore.savePlaybackPosition({
      song_id: song.id,
      position_seconds: positionRef.current,
      duration_seconds:
        durationRef.current > 0 ? durationRef.current : (song.duration ?? 0),
      is_playing: playing,
    });
  }, []);

  useEffect(() => {
    let mounted = true;

    async function initializePlayback() {
      await ensurePlayerReady();
      const playbackSnapshot = await stereodromeCore.getPlaybackSnapshot();

      if (!mounted) {
        return;
      }

      applyPlaybackSnapshot(playbackSnapshot);
    }

    void initializePlayback().catch((setupError: unknown) => {
      if (mounted) {
        setError(errorMessage(setupError));
      }
    });

    const unsubscribePlayback = stereodromeCore.addEventListener(
      "playback-snapshot",
      (snapshot) => {
        if (AppState.currentState === "active") {
          applyPlaybackSnapshot(snapshot);
        }
      }
    );

    return () => {
      mounted = false;
      unsubscribePlayback();
    };
  }, [applyPlaybackSnapshot]);

  useEffect(() => {
    function handleAppStateChange(nextState: AppStateStatus) {
      const active = nextState === "active";
      if (active) {
        void reconcilePlaybackSnapshot().catch((playbackError: unknown) => {
          setError(errorMessage(playbackError));
        });
      } else {
        void persistPlaybackPosition(isPlayingRef.current).catch(
          (playbackError: unknown) => {
            setError(errorMessage(playbackError));
          }
        );
      }
    }

    const subscription = AppState.addEventListener(
      "change",
      handleAppStateChange
    );
    return () => {
      subscription.remove();
    };
  }, [persistPlaybackPosition, reconcilePlaybackSnapshot]);

  useEffect(() => {
    let cancelled = false;
    let timeout: ReturnType<typeof setTimeout> | null = null;

    const clearScheduledPoll = () => {
      if (timeout !== null) {
        clearTimeout(timeout);
        timeout = null;
      }
    };

    const schedulePoll = () => {
      clearScheduledPoll();
      if (cancelled || AppState.currentState !== "active") {
        return;
      }
      timeout = setTimeout(poll, currentSongRef.current ? 30_000 : 60_000);
    };

    const poll = () => {
      void reconcilePlaybackSnapshot()
        .catch((playbackError: unknown) => {
          if (!cancelled) {
            setError(errorMessage(playbackError));
          }
        })
        .finally(() => {
          if (cancelled) {
            return;
          }
          schedulePoll();
        });
    };

    schedulePoll();
    const appStateSubscription = AppState.addEventListener("change", () => {
      schedulePoll();
    });

    return () => {
      cancelled = true;
      clearScheduledPoll();
      appStateSubscription.remove();
    };
  }, [reconcilePlaybackSnapshot]);

  useEffect(() => {
    if (!isPlaying) {
      return undefined;
    }
    const interval = setInterval(() => {
      const nextPosition =
        durationRef.current > 0
          ? Math.min(durationRef.current, positionRef.current + 1)
          : positionRef.current + 1;
      positionRef.current = nextPosition;
      setPosition(nextPosition);
    }, 1000);
    return () => {
      clearInterval(interval);
    };
  }, [isPlaying]);

  const playSong = useCallback(
    async (song: PlayableSong, songs: PlayableSong[] = [song]) => {
      await runPlaybackAction("transport", async () => {
        setError(null);
        setCurrentSong(song);
        await ensurePlayerReady();

        await stereodromeCore.playSongWithQueue(
          song.id,
          songs.map((candidate) => candidate.id)
        );
        await reconcilePlaybackSnapshot();
      });
    },
    [reconcilePlaybackSnapshot, runPlaybackAction]
  );

  const toggle = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await ensurePlayerReady();
      await stereodromeCore.togglePlayback();
      await reconcilePlaybackSnapshot();
    });
  }, [reconcilePlaybackSnapshot, runPlaybackAction]);

  const toggleRepeat = useCallback(async () => {
    await runPlaybackAction("queue", async () => {
      await ensurePlayerReady();
      await stereodromeCore.cycleRepeatMode();
    });
  }, [runPlaybackAction]);

  const rerollNext = useCallback(async () => {
    await runPlaybackAction("queue", async () => {
      await ensurePlayerReady();
      await stereodromeCore.rerollNext();
    });
  }, [runPlaybackAction]);

  const toggleShuffle = useCallback(async () => {
    await runPlaybackAction("queue", async () => {
      await ensurePlayerReady();
      await stereodromeCore.toggleShuffle();
    });
  }, [runPlaybackAction]);

  const seekBy = useCallback(
    async (seconds: number) => {
      if (!Number.isFinite(seconds) || seconds === 0) {
        return;
      }
      await ensurePlayerReady();
      await stereodromeCore.seekBy(seconds);
      await reconcilePlaybackSnapshot();
    },
    [reconcilePlaybackSnapshot]
  );

  const next = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await ensurePlayerReady();
      await stereodromeCore.audioPlayNext(true);
      await finalizePlaybackStart();
    });
  }, [finalizePlaybackStart, runPlaybackAction]);

  const previous = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await ensurePlayerReady();
      await stereodromeCore.audioPlayPrevious();
      await finalizePlaybackStart();
    });
  }, [finalizePlaybackStart, runPlaybackAction]);

  const playQueueIndex = useCallback(
    async (index: number) => {
      await runPlaybackAction("transport", async () => {
        await ensurePlayerReady();
        await stereodromeCore.audioPlayQueueItem(index);
        await finalizePlaybackStart();
      });
    },
    [finalizePlaybackStart, runPlaybackAction]
  );

  const removeQueueIndex = useCallback(
    async (index: number) => {
      await runPlaybackAction("queue", async () => {
        await stereodromeCore.removeFromQueue(index);
      });
    },
    [runPlaybackAction]
  );

  const clearQueue = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await stereodromeCore.clearQueue();
    });
  }, [runPlaybackAction]);

  const nextSong = getNextSong({
    currentIndex,
    pendingNavigationIndex,
    preparedNextSong,
    queue,
    repeatMode,
  });

  const actions = useMemo(
    () => ({
      clearQueue,
      next,
      playQueueIndex,
      playSong,
      previous,
      removeQueueIndex,
      rerollNext,
      seekBy,
      toggle,
      toggleRepeat,
      toggleShuffle,
    }),
    [
      clearQueue,
      next,
      playQueueIndex,
      playSong,
      previous,
      removeQueueIndex,
      rerollNext,
      seekBy,
      toggle,
      toggleRepeat,
      toggleShuffle,
    ]
  );

  const value = useMemo(
    () => ({
      currentSong,
      duration,
      error,
      isPlaying,
      nextSong,
      position,
      queue,
      repeatMode,
      repeatEnabled: repeatMode !== "Off",
      shuffleEnabled,
      ...actions,
    }),
    [
      currentSong,
      duration,
      error,
      isPlaying,
      nextSong,
      position,
      queue,
      repeatMode,
      shuffleEnabled,
      actions,
    ]
  );

  const metadata = useMemo(
    () => ({
      currentSong,
      error,
      isPlaying,
      nextSong,
      queue,
      repeatMode,
      repeatEnabled: repeatMode !== "Off",
      shuffleEnabled,
    }),
    [currentSong, error, isPlaying, nextSong, queue, repeatMode, shuffleEnabled]
  );

  return (
    <PlaybackActionsContext.Provider value={actions}>
      <PlaybackMetadataContext.Provider value={metadata}>
        <PlaybackContext.Provider value={value}>
          {children}
        </PlaybackContext.Provider>
      </PlaybackMetadataContext.Provider>
    </PlaybackActionsContext.Provider>
  );
}

export function usePlaybackMetadata() {
  const context = useContext(PlaybackMetadataContext);
  if (!context) {
    throw new Error("usePlaybackMetadata must be used within PlaybackProvider");
  }
  return context;
}

export function usePlaybackActions() {
  const context = useContext(PlaybackActionsContext);
  if (!context) {
    throw new Error("usePlaybackActions must be used within PlaybackProvider");
  }
  return context;
}

export function usePlayback() {
  const context = useContext(PlaybackContext);
  if (!context) {
    throw new Error("usePlayback must be used within PlaybackProvider");
  }
  return context;
}
