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
import { nativeMediaControls } from "@/services/nativeMediaControls";
import type {
  AudioPlaybackStatus,
  AudioProcessingSettings,
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
  const [playbackActivatedThisProcess, setPlaybackActivatedThisProcess] =
    useState(false);
  const [isAppActive, setIsAppActive] = useState(
    AppState.currentState === "active"
  );
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const currentIndexRef = useRef<number | null>(null);
  const pendingNavigationIndexRef = useRef<number | null>(null);
  const currentSongRef = useRef<PlayableSong | null>(null);
  const queueRef = useRef<PlayableSong[]>([]);
  const isPlayingRef = useRef(false);
  const positionRef = useRef(0);
  const durationRef = useRef(0);
  const restoredStartPositionRef = useRef<number | null>(null);
  const expectedAudioSongIdRef = useRef<string | null>(null);
  const actionLocksRef = useRef(new Set<string>());

  const activatePlaybackSession = useCallback(() => {
    setPlaybackActivatedThisProcess(true);
  }, []);

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

  const updateAudioStatus = useCallback(
    (status: AudioPlaybackStatus, songs = queueRef.current) => {
      const expectedAudioSongId = expectedAudioSongIdRef.current;
      if (
        expectedAudioSongId &&
        status.current_song_id !== expectedAudioSongId
      ) {
        isPlayingRef.current = status.is_playing;
        setIsPlaying(status.is_playing);
        return;
      }
      if (expectedAudioSongId) {
        expectedAudioSongIdRef.current = null;
      }

      isPlayingRef.current = status.is_playing;
      setIsPlaying(status.is_playing);

      if (status.current_song_id) {
        positionRef.current = status.position;
        durationRef.current = status.duration;
        setPosition(status.position);
        setDuration(status.duration);
        const song =
          songs.find((candidate) => candidate.id === status.current_song_id) ??
          currentSongRef.current;
        currentSongRef.current = song;
        setCurrentSong(song);
      } else if (currentIndexRef.current === null) {
        positionRef.current = 0;
        durationRef.current = 0;
        setPosition(0);
        setDuration(0);
        currentSongRef.current = null;
        setCurrentSong(null);
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
    pendingNavigationIndexRef.current = pendingIndex;
    queueRef.current = songs;
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

  const prepareNextPlayback = useCallback(async () => {
    await stereodromeCore.prefetchNext();
    await stereodromeCore.audioPrepareNextTransition();
  }, []);

  const playCurrentQueueItem = useCallback(
    async (startPositionSeconds?: number) => {
      expectedAudioSongIdRef.current = currentSongRef.current?.id ?? null;
      try {
        let status = await stereodromeCore.audioPlayCurrent();

        if (
          startPositionSeconds !== undefined &&
          Number.isFinite(startPositionSeconds) &&
          startPositionSeconds > 0
        ) {
          const seekPosition =
            status.duration > 0
              ? Math.min(startPositionSeconds, status.duration)
              : startPositionSeconds;
          await stereodromeCore.audioSeek(Math.max(0, seekPosition));
          status = await stereodromeCore.audioGetStatus();
        }

        restoredStartPositionRef.current = null;
        updateAudioStatus(status);
        void prepareNextPlayback().catch(() => {});
      } finally {
        expectedAudioSongIdRef.current = null;
      }
    },
    [prepareNextPlayback, updateAudioStatus]
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
        await applyQueueState(nextQueueState);
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
    [applyQueueState]
  );

  const applyAudioProcessingSettings = useCallback(async () => {
    await ensurePlayerReady();
    const status = await stereodromeCore.audioApplySettings();
    updateAudioStatus(status);
    void prepareNextPlayback().catch(() => {});
  }, [prepareNextPlayback, updateAudioStatus]);

  const refreshFromNativePlayback = useCallback(async () => {
    const state = await stereodromeCore.getQueue();
    const songs = state.items.map(playableFromQueueItem);
    await applyQueueState(state);
    const status = await stereodromeCore.audioGetStatus();
    updateAudioStatus(status, songs);
  }, [applyQueueState, updateAudioStatus]);

  useEffect(() => {
    let mounted = true;

    async function initializePlayback() {
      await ensurePlayerReady();
      const [queueState, playbackState] = await Promise.all([
        stereodromeCore.getQueue(),
        stereodromeCore.getPlaybackState(),
      ]);

      if (!mounted) {
        return;
      }

      await applyQueueState(queueState);
      await applyAudioProcessingSettings();
      await restorePlaybackState(queueState, playbackState);
    }

    void initializePlayback().catch((setupError) => {
      if (mounted) {
        setError(errorMessage(setupError));
      }
    });

    const unsubscribeQueue = stereodromeCore.addEventListener<QueueState>(
      "queue-changed",
      (state) => {
        void applyQueueState(state).catch((playbackError) => {
          setError(errorMessage(playbackError));
        });
      }
    );
    const unsubscribeAudioSettings =
      stereodromeCore.addEventListener<AudioProcessingSettings>(
        "audio-processing-settings-changed",
        () => {
          void applyAudioProcessingSettings().catch((playbackError) => {
            setError(errorMessage(playbackError));
          });
        }
      );

    return () => {
      mounted = false;
      unsubscribeQueue();
      unsubscribeAudioSettings();
    };
  }, [applyAudioProcessingSettings, applyQueueState, restorePlaybackState]);

  useEffect(() => {
    return nativeMediaControls.addInvalidatedListener(() => {
      void refreshFromNativePlayback().catch((playbackError) => {
        setError(errorMessage(playbackError));
      });
    });
  }, [refreshFromNativePlayback]);

  useEffect(() => {
    function handleAppStateChange(nextState: AppStateStatus) {
      const active = nextState === "active";
      setIsAppActive(active);
      if (active) {
        void refreshFromNativePlayback().catch((playbackError) => {
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
  }, [persistPlaybackPosition, refreshFromNativePlayback]);

  useEffect(() => {
    let cancelled = false;
    let timeout: ReturnType<typeof setTimeout> | null = null;

    const poll = () => {
      void stereodromeCore
        .audioGetStatus()
        .then(async (status) => {
          if (cancelled) {
            return;
          }

          const state = await stereodromeCore.getQueue();
          const songs = state.items.map(playableFromQueueItem);
          await applyQueueState(state);
          updateAudioStatus(status, songs);
          if (expectedAudioSongIdRef.current) {
            return;
          }

          if (!status.current_song_id && state.current_index === null) {
            await nativeMediaControls.clear();
          }
        })
        .catch((playbackError) => {
          if (!cancelled) {
            setError(errorMessage(playbackError));
          }
        })
        .finally(() => {
          if (cancelled) {
            return;
          }
          const delay = isPlayingRef.current
            ? isAppActive
              ? 1_000
              : 5_000
            : currentSongRef.current
              ? 5_000
              : 15_000;
          timeout = setTimeout(poll, delay);
        });
    };

    poll();

    return () => {
      cancelled = true;
      if (timeout) {
        clearTimeout(timeout);
      }
    };
  }, [applyQueueState, isAppActive, updateAudioStatus]);

  const playSong = useCallback(
    async (song: PlayableSong, songs: PlayableSong[] = [song]) => {
      await runPlaybackAction("transport", async () => {
        setError(null);
        setCurrentSong(song);
        await ensurePlayerReady();

        const state = await stereodromeCore.playSongWithQueue(
          song.id,
          songs.map((candidate) => candidate.id)
        );
        await applyQueueState(state);
        activatePlaybackSession();
        await playCurrentQueueItem();
      });
    },
    [
      activatePlaybackSession,
      applyQueueState,
      playCurrentQueueItem,
      runPlaybackAction,
    ]
  );

  const toggle = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await ensurePlayerReady();
      if (isPlayingRef.current) {
        await stereodromeCore.audioPause();
        const status = await stereodromeCore.audioGetStatus();
        updateAudioStatus(status);
        await persistPlaybackPosition(false);
      } else if (currentIndexRef.current !== null) {
        const status = await stereodromeCore.audioGetStatus();
        if (status.current_song_id) {
          activatePlaybackSession();
          await stereodromeCore.audioResume();
          updateAudioStatus(await stereodromeCore.audioGetStatus());
        } else {
          activatePlaybackSession();
          await playCurrentQueueItem(
            restoredStartPositionRef.current ?? positionRef.current
          );
        }
      }
    });
  }, [
    activatePlaybackSession,
    persistPlaybackPosition,
    playCurrentQueueItem,
    runPlaybackAction,
    updateAudioStatus,
  ]);

  const toggleRepeat = useCallback(async () => {
    await runPlaybackAction("queue", async () => {
      await ensurePlayerReady();
      const state = await stereodromeCore.cycleRepeatMode();
      await applyQueueState(state);
    });
  }, [applyQueueState, runPlaybackAction]);

  const rerollNext = useCallback(async () => {
    await runPlaybackAction("queue", async () => {
      await ensurePlayerReady();
      const state = await stereodromeCore.rerollNext();
      await applyQueueState(state);
    });
  }, [applyQueueState, runPlaybackAction]);

  const toggleShuffle = useCallback(async () => {
    await runPlaybackAction("queue", async () => {
      await ensurePlayerReady();
      const state = await stereodromeCore.toggleShuffle();
      await applyQueueState(state);
    });
  }, [applyQueueState, runPlaybackAction]);

  const seekBy = useCallback(
    async (seconds: number) => {
      try {
        await ensurePlayerReady();
        const status = await stereodromeCore.audioGetStatus();
        const basePosition = status.current_song_id
          ? status.position
          : positionRef.current;
        const baseDuration = status.current_song_id
          ? status.duration
          : durationRef.current;
        const nextPosition = Math.max(
          0,
          baseDuration > 0
            ? Math.min(baseDuration, basePosition + seconds)
            : basePosition + seconds
        );
        if (status.current_song_id) {
          await stereodromeCore.audioSeek(nextPosition);
        } else {
          restoredStartPositionRef.current = nextPosition;
        }
        positionRef.current = nextPosition;
        setPosition(nextPosition);
        await persistPlaybackPosition(isPlayingRef.current);
      } catch (playbackError) {
        setError(errorMessage(playbackError));
      }
    },
    [persistPlaybackPosition]
  );

  const next = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await ensurePlayerReady();
      const state = await stereodromeCore.playNext(true);
      await applyQueueState(state);
      activatePlaybackSession();
      await playCurrentQueueItem();
    });
  }, [
    activatePlaybackSession,
    applyQueueState,
    playCurrentQueueItem,
    runPlaybackAction,
  ]);

  const previous = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      await ensurePlayerReady();
      const state = await stereodromeCore.playPrevious();
      await applyQueueState(state);
      activatePlaybackSession();
      await playCurrentQueueItem();
    });
  }, [
    activatePlaybackSession,
    applyQueueState,
    playCurrentQueueItem,
    runPlaybackAction,
  ]);

  const playQueueIndex = useCallback(
    async (index: number) => {
      await runPlaybackAction("transport", async () => {
        const state = await stereodromeCore.playQueueItem(index);
        await applyQueueState(state);
        activatePlaybackSession();
        await playCurrentQueueItem();
      });
    },
    [
      activatePlaybackSession,
      applyQueueState,
      playCurrentQueueItem,
      runPlaybackAction,
    ]
  );

  const removeQueueIndex = useCallback(
    async (index: number) => {
      await runPlaybackAction("queue", async () => {
        const state = await stereodromeCore.removeFromQueue(index);
        await applyQueueState(state);
      });
    },
    [applyQueueState, runPlaybackAction]
  );

  const clearQueue = useCallback(async () => {
    await runPlaybackAction("transport", async () => {
      const state = await stereodromeCore.clearQueue();
      expectedAudioSongIdRef.current = null;
      restoredStartPositionRef.current = null;
      setPlaybackActivatedThisProcess(false);
      await stereodromeCore.audioStop();
      await applyQueueState(state);
      await nativeMediaControls.clear();
      positionRef.current = 0;
      durationRef.current = 0;
      setPosition(0);
      setDuration(0);
      setIsPlaying(false);
    });
  }, [applyQueueState, runPlaybackAction]);

  const nextSong = getNextSong({
    currentIndex,
    pendingNavigationIndex,
    preparedNextSong,
    queue,
    repeatMode,
  });

  useEffect(() => {
    void nativeMediaControls
      .sync({
        currentSong,
        canPlay: playbackActivatedThisProcess,
        currentIndex,
        duration,
        isPlaying,
        nextSong,
        position,
        queue,
      })
      .catch(() => {});
  }, [
    currentSong,
    playbackActivatedThisProcess,
    currentIndex,
    duration,
    isPlaying,
    nextSong,
    position,
    queue,
  ]);

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
