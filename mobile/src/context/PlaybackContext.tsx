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

import {
  isPlaybackSnapshot,
  stereodromeCore,
} from "@/services/stereodromeCore";
import type {
  PlaybackSnapshot,
  PlaybackStateSnapshot,
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

const PlaybackContext = createContext<PlaybackContextValue | null>(null);

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
  const isPlayingRef = useRef(false);
  const canPlayRef = useRef(false);
  const audioLoadedRef = useRef(false);
  const positionRef = useRef(0);
  const durationRef = useRef(0);
  const restoredStartPositionRef = useRef<number | null>(null);
  const lastSnapshotSeqRef = useRef(0);
  const actionLocksRef = useRef(new Set<string>());
  const pendingSeekDeltasRef = useRef<number[]>([]);
  const seekDrainRef = useRef<Promise<void> | null>(null);

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
    const songs = state.items.map(playableFromQueueItem);
    const preparedSong = state.prepared_next_item
      ? playableFromQueueItem(state.prepared_next_item)
      : null;
    const index = state.current_index;
    const pendingIndex = state.pending_navigation_index;
    const removedCurrentIsStillPlaying =
      index === null && pendingIndex !== null;

    setQueue(songs);
    setCurrentIndex(index);
    setPendingNavigationIndex(pendingIndex);
    setPreparedNextSong(preparedSong);
    if (removedCurrentIsStillPlaying) {
      setCurrentSong(currentSongRef.current);
    } else {
      const nextCurrentSong = index === null ? null : (songs[index] ?? null);
      currentSongRef.current = nextCurrentSong;
      setCurrentSong(nextCurrentSong);
    }
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

      const current =
        snapshot.song === null
          ? null
          : {
              id: snapshot.song.id,
              title: snapshot.song.title,
              artist: snapshot.song.artist,
              album: snapshot.song.album,
              duration: snapshot.song.duration_seconds,
            };

      audioLoadedRef.current = snapshot.audio_loaded;
      isPlayingRef.current = snapshot.is_playing;
      canPlayRef.current = snapshot.can_play;
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

  const prepareNextPlayback = useCallback(async () => {
    await stereodromeCore.audioPrepareNextTransition();
  }, []);

  const finalizePlaybackStart = useCallback(
    async (startPositionSeconds?: number) => {
      await reconcilePlaybackSnapshot();

      if (
        startPositionSeconds !== undefined &&
        Number.isFinite(startPositionSeconds) &&
        startPositionSeconds > 0
      ) {
        const seekPosition =
          durationRef.current > 0
            ? Math.min(startPositionSeconds, durationRef.current)
            : startPositionSeconds;
        await stereodromeCore.audioSeek(Math.max(0, seekPosition));
        await reconcilePlaybackSnapshot();
      }

      restoredStartPositionRef.current = null;
      prepareNextPlayback().catch(() => {
        // Preparing the next item is opportunistic; playback already succeeded.
      });
    },
    [prepareNextPlayback, reconcilePlaybackSnapshot]
  );

  const playCurrentQueueItem = useCallback(
    async (startPositionSeconds?: number) => {
      await stereodromeCore.audioPlayCurrent();
      await finalizePlaybackStart(startPositionSeconds);
    },
    [finalizePlaybackStart]
  );

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

  const restorePlaybackState = useCallback(
    async (queueState: QueueState, snapshot: PlaybackStateSnapshot) => {
      const restoredSongId = snapshot.current_song_id;
      if (restoredSongId === null || restoredSongId.length === 0) {
        restoredStartPositionRef.current = null;
        return queueState;
      }

      const restoredIndex = queueState.items.findIndex(
        (item) => item.song_id === restoredSongId
      );
      if (restoredIndex === -1) {
        restoredStartPositionRef.current = null;
        return queueState;
      }

      let nextQueueState = queueState;
      if (queueState.current_index !== restoredIndex) {
        nextQueueState = await stereodromeCore.playQueueItem(restoredIndex);
      }

      const restoredPosition = Math.max(0, snapshot.position_seconds);
      const restoredDuration =
        snapshot.duration_seconds > 0
          ? snapshot.duration_seconds
          : (nextQueueState.items[restoredIndex]?.duration ?? 0);
      positionRef.current = restoredPosition;
      durationRef.current = restoredDuration;
      restoredStartPositionRef.current = restoredPosition;
      isPlayingRef.current = false;
      setPosition(restoredPosition);
      setDuration(restoredDuration);
      setIsPlaying(false);

      if (snapshot.was_playing) {
        await stereodromeCore.savePlaybackPosition({
          song_id: restoredSongId,
          position_seconds: restoredPosition,
          duration_seconds: restoredDuration,
          is_playing: false,
        });
      }

      return nextQueueState;
    },
    []
  );

  const applyAudioProcessingSettings = useCallback(async () => {
    await ensurePlayerReady();
    await stereodromeCore.audioApplySettings();
    await reconcilePlaybackSnapshot();
    prepareNextPlayback().catch(() => {
      // Applying settings should not fail when optional preloading fails.
    });
  }, [prepareNextPlayback, reconcilePlaybackSnapshot]);

  useEffect(() => {
    let mounted = true;

    async function initializePlayback() {
      await ensurePlayerReady();
      const [playbackSnapshot, playbackState] = await Promise.all([
        stereodromeCore.getPlaybackSnapshot(),
        stereodromeCore.getPlaybackState(),
      ]);

      if (!mounted) {
        return;
      }

      applyPlaybackSnapshot(playbackSnapshot);
      await applyAudioProcessingSettings();
      await restorePlaybackState(playbackSnapshot.queue, playbackState);
      await reconcilePlaybackSnapshot();
    }

    void initializePlayback().catch((setupError: unknown) => {
      if (mounted) {
        setError(errorMessage(setupError));
      }
    });

    const unsubscribePlayback = stereodromeCore.addEventListener(
      "playback-snapshot",
      (snapshot) => {
        applyPlaybackSnapshot(snapshot);
      },
      isPlaybackSnapshot
    );

    return () => {
      mounted = false;
      unsubscribePlayback();
    };
  }, [
    applyAudioProcessingSettings,
    applyPlaybackSnapshot,
    reconcilePlaybackSnapshot,
    restorePlaybackState,
  ]);

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
          timeout = setTimeout(poll, currentSongRef.current ? 30_000 : 60_000);
        });
    };

    poll();

    return () => {
      cancelled = true;
      if (timeout !== null) {
        clearTimeout(timeout);
      }
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
        await playCurrentQueueItem();
      });
    },
    [playCurrentQueueItem, runPlaybackAction]
  );

  const toggle = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await ensurePlayerReady();
      if (isPlayingRef.current) {
        await stereodromeCore.audioPause();
        await reconcilePlaybackSnapshot();
        await persistPlaybackPosition(false);
      } else if (canPlayRef.current) {
        await stereodromeCore.audioResume();
        restoredStartPositionRef.current = null;
        await reconcilePlaybackSnapshot();
      }
    });
  }, [persistPlaybackPosition, reconcilePlaybackSnapshot, runPlaybackAction]);

  const toggleRepeat = useCallback(async () => {
    await runPlaybackAction("queue", async () => {
      await ensurePlayerReady();
      await stereodromeCore.cycleRepeatMode();
      await reconcilePlaybackSnapshot();
    });
  }, [reconcilePlaybackSnapshot, runPlaybackAction]);

  const rerollNext = useCallback(async () => {
    await runPlaybackAction("queue", async () => {
      await ensurePlayerReady();
      await stereodromeCore.rerollNext();
      await reconcilePlaybackSnapshot();
    });
  }, [reconcilePlaybackSnapshot, runPlaybackAction]);

  const toggleShuffle = useCallback(async () => {
    await runPlaybackAction("queue", async () => {
      await ensurePlayerReady();
      await stereodromeCore.toggleShuffle();
      await reconcilePlaybackSnapshot();
    });
  }, [reconcilePlaybackSnapshot, runPlaybackAction]);

  const drainPendingSeeks = useCallback(async () => {
    await ensurePlayerReady();

    do {
      const deltas = pendingSeekDeltasRef.current.splice(0);
      const seekDuration = durationRef.current;
      let nextPosition = positionRef.current;

      for (const delta of deltas) {
        nextPosition = Math.max(
          0,
          seekDuration > 0
            ? Math.min(seekDuration, nextPosition + delta)
            : nextPosition + delta
        );
      }

      positionRef.current = nextPosition;
      setPosition(nextPosition);
      if (audioLoadedRef.current) {
        // oxlint-disable-next-line no-await-in-loop -- Serialize coalesced native seeks.
        await stereodromeCore.audioSeek(nextPosition);
        // oxlint-disable-next-line no-await-in-loop -- Base the next batch on authoritative state.
        await reconcilePlaybackSnapshot();
      } else {
        restoredStartPositionRef.current = nextPosition;
      }

      // oxlint-disable-next-line no-await-in-loop -- Persist each coalesced batch before draining the next.
      await persistPlaybackPosition(isPlayingRef.current);
    } while (pendingSeekDeltasRef.current.length > 0);
  }, [persistPlaybackPosition, reconcilePlaybackSnapshot]);

  const seekBy = useCallback(
    async (seconds: number) => {
      if (!Number.isFinite(seconds) || seconds === 0) {
        return;
      }

      pendingSeekDeltasRef.current.push(seconds);
      seekDrainRef.current ??= drainPendingSeeks()
        .catch((playbackError: unknown) => {
          pendingSeekDeltasRef.current = [];
          setError(errorMessage(playbackError));
        })
        .finally(() => {
          seekDrainRef.current = null;
        });

      await seekDrainRef.current;
    },
    [drainPendingSeeks]
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
        await reconcilePlaybackSnapshot();
      });
    },
    [reconcilePlaybackSnapshot, runPlaybackAction]
  );

  const clearQueue = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await stereodromeCore.clearQueue();
      restoredStartPositionRef.current = null;
      await stereodromeCore.audioStop();
      await reconcilePlaybackSnapshot();
    });
  }, [reconcilePlaybackSnapshot, runPlaybackAction]);

  const nextSong = getNextSong({
    currentIndex,
    pendingNavigationIndex,
    preparedNextSong,
    queue,
    repeatMode,
  });

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
      clearQueue,
      playSong,
      playQueueIndex,
      removeQueueIndex,
      toggle,
      toggleRepeat,
      toggleShuffle,
      rerollNext,
      seekBy,
      next,
      previous,
    }),
    [
      currentSong,
      duration,
      error,
      isPlaying,
      next,
      nextSong,
      position,
      previous,
      queue,
      repeatMode,
      shuffleEnabled,
      clearQueue,
      playSong,
      playQueueIndex,
      removeQueueIndex,
      toggle,
      toggleRepeat,
      toggleShuffle,
      rerollNext,
      seekBy,
    ]
  );

  return (
    <PlaybackContext.Provider value={value}>
      {children}
    </PlaybackContext.Provider>
  );
}

export function usePlayback() {
  const context = useContext(PlaybackContext);
  if (!context) {
    throw new Error("usePlayback must be used within PlaybackProvider");
  }
  return context;
}
