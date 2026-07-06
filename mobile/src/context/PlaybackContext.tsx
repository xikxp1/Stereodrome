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
  AudioProcessingSettings,
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
  const currentIndexRef = useRef<number | null>(null);
  const currentSongRef = useRef<PlayableSong | null>(null);
  const isPlayingRef = useRef(false);
  const audioLoadedRef = useRef(false);
  const positionRef = useRef(0);
  const durationRef = useRef(0);
  const restoredStartPositionRef = useRef<number | null>(null);
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

  const applyQueueState = useCallback(async (state: QueueState) => {
    const songs = state.items.map(playableFromQueueItem);
    const preparedSong = state.prepared_next_item
      ? playableFromQueueItem(state.prepared_next_item)
      : null;
    const index = state.current_index;
    const pendingIndex = state.pending_navigation_index;
    const removedCurrentIsStillPlaying =
      index === null && pendingIndex !== null;

    currentIndexRef.current = index;
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
    async (snapshot: PlaybackSnapshot) => {
      if (snapshot.seq <= lastSnapshotSeqRef.current) {
        return;
      }
      lastSnapshotSeqRef.current = snapshot.seq;

      await applyQueueState(snapshot.queue);

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
    await applyPlaybackSnapshot(await stereodromeCore.getPlaybackSnapshot());
  }, [applyPlaybackSnapshot]);

  const prepareNextPlayback = useCallback(async () => {
    await stereodromeCore.prefetchNext();
    await stereodromeCore.audioPrepareNextTransition();
  }, []);

  const playCurrentQueueItem = useCallback(
    async (startPositionSeconds?: number) => {
      await stereodromeCore.audioPlayCurrent();
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
      void prepareNextPlayback().catch(() => {});
    },
    [prepareNextPlayback, reconcilePlaybackSnapshot]
  );

  const persistPlaybackPosition = useCallback(async (isPlaying: boolean) => {
    const song = currentSongRef.current;
    if (!song) {
      return;
    }

    await stereodromeCore.savePlaybackPosition({
      song_id: song.id,
      position_seconds: positionRef.current,
      duration_seconds: durationRef.current || song.duration || 0,
      is_playing: isPlaying,
    });
  }, []);

  const restorePlaybackState = useCallback(
    async (queueState: QueueState, snapshot: PlaybackStateSnapshot) => {
      const restoredSongId = snapshot.current_song_id;
      if (!restoredSongId) {
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
        snapshot.duration_seconds ||
        nextQueueState.items[restoredIndex]?.duration ||
        0;
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
    void prepareNextPlayback().catch(() => {});
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

      await applyPlaybackSnapshot(playbackSnapshot);
      await applyAudioProcessingSettings();
      await restorePlaybackState(playbackSnapshot.queue, playbackState);
      await reconcilePlaybackSnapshot();
    }

    void initializePlayback().catch((setupError) => {
      if (mounted) {
        setError(errorMessage(setupError));
      }
    });

    const unsubscribeAudioSettings =
      stereodromeCore.addEventListener<AudioProcessingSettings>(
        "audio-processing-settings-changed",
        () => {
          void applyAudioProcessingSettings().catch((playbackError) => {
            setError(errorMessage(playbackError));
          });
        }
      );
    const unsubscribePlayback =
      stereodromeCore.addEventListener<PlaybackSnapshot>(
        "playback-snapshot",
        (snapshot) => {
          void applyPlaybackSnapshot(snapshot).catch((playbackError) => {
            setError(errorMessage(playbackError));
          });
        }
      );

    return () => {
      mounted = false;
      unsubscribeAudioSettings();
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
        void reconcilePlaybackSnapshot().catch((playbackError) => {
          setError(errorMessage(playbackError));
        });
      } else {
        void persistPlaybackPosition(isPlayingRef.current).catch(
          (playbackError) => {
            setError(errorMessage(playbackError));
          }
        );
      }
    }

    const subscription = AppState.addEventListener(
      "change",
      handleAppStateChange
    );
    return () => subscription.remove();
  }, [persistPlaybackPosition, reconcilePlaybackSnapshot]);

  useEffect(() => {
    let cancelled = false;
    let timeout: ReturnType<typeof setTimeout> | null = null;

    const poll = () => {
      void reconcilePlaybackSnapshot()
        .catch((playbackError) => {
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
      if (timeout) {
        clearTimeout(timeout);
      }
    };
  }, [reconcilePlaybackSnapshot]);

  useEffect(() => {
    if (!isPlaying) {
      return;
    }
    const interval = setInterval(() => {
      const nextPosition =
        durationRef.current > 0
          ? Math.min(durationRef.current, positionRef.current + 1)
          : positionRef.current + 1;
      positionRef.current = nextPosition;
      setPosition(nextPosition);
    }, 1000);
    return () => clearInterval(interval);
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
      } else if (currentIndexRef.current !== null) {
        await stereodromeCore.audioResume();
        restoredStartPositionRef.current = null;
        await reconcilePlaybackSnapshot();
      }
    });
  }, [
    persistPlaybackPosition,
    reconcilePlaybackSnapshot,
    runPlaybackAction,
  ]);

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

  const seekBy = useCallback(
    async (seconds: number) => {
      try {
        await ensurePlayerReady();
        const basePosition = positionRef.current;
        const baseDuration = durationRef.current;
        const nextPosition = Math.max(
          0,
          baseDuration > 0
            ? Math.min(baseDuration, basePosition + seconds)
            : basePosition + seconds
        );
        if (audioLoadedRef.current) {
          await stereodromeCore.audioSeek(nextPosition);
          await reconcilePlaybackSnapshot();
        } else {
          restoredStartPositionRef.current = nextPosition;
          positionRef.current = nextPosition;
          setPosition(nextPosition);
        }
        await persistPlaybackPosition(isPlayingRef.current);
      } catch (playbackError) {
        setError(errorMessage(playbackError));
      }
    },
    [persistPlaybackPosition, reconcilePlaybackSnapshot]
  );

  const next = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await ensurePlayerReady();
      await stereodromeCore.playNext(true);
      await playCurrentQueueItem();
    });
  }, [playCurrentQueueItem, runPlaybackAction]);

  const previous = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await ensurePlayerReady();
      await stereodromeCore.playPrevious();
      await playCurrentQueueItem();
    });
  }, [playCurrentQueueItem, runPlaybackAction]);

  const playQueueIndex = useCallback(
    async (index: number) => {
      await runPlaybackAction("transport", async () => {
        await stereodromeCore.playQueueItem(index);
        await playCurrentQueueItem();
      });
    },
    [playCurrentQueueItem, runPlaybackAction]
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
