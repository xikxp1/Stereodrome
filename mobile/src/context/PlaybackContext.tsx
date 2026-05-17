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

export function PlaybackProvider({ children }: { children: React.ReactNode }) {
  const [error, setError] = useState<string | null>(null);
  const [currentSong, setCurrentSong] = useState<PlayableSong | null>(null);
  const [currentIndex, setCurrentIndex] = useState<number | null>(null);
  const [pendingNavigationIndex, setPendingNavigationIndex] = useState<
    number | null
  >(null);
  const [queue, setQueue] = useState<PlayableSong[]>([]);
  const [repeatMode, setRepeatMode] =
    useState<QueueState["repeat_mode"]>("Off");
  const [shuffleEnabled, setShuffleEnabled] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);
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
  const handlingEndedRef = useRef(false);
  const crossfadeInProgressRef = useRef(false);
  const crossfadeAttemptedSongIdRef = useRef<string | null>(null);
  const expectedAudioSongIdRef = useRef<string | null>(null);
  const audioProcessingSettingsRef = useRef<AudioProcessingSettings | null>(
    null
  );
  const lastProgressReportRef = useRef<{
    at: number;
    isPlaying: boolean;
    position: number;
    songId: string;
  } | null>(null);

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
      crossfadeAttemptedSongIdRef.current = null;
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
        await playCurrentQueueItem(restoredPosition);
      }

      return nextQueueState;
    },
    [applyQueueState, playCurrentQueueItem]
  );

  const applyAudioProcessingSettings = useCallback(
    async (settings: AudioProcessingSettings) => {
      audioProcessingSettingsRef.current = settings;
      await ensurePlayerReady();
      const status = await stereodromeCore.audioApplySettings();
      updateAudioStatus(status);
      void prepareNextPlayback().catch(() => {});
    },
    [prepareNextPlayback, updateAudioStatus]
  );

  const handlePlaybackEnded = useCallback(async () => {
    if (handlingEndedRef.current) {
      return;
    }

    handlingEndedRef.current = true;
    try {
      const state = await stereodromeCore.playNext(false);
      await applyQueueState(state);
      if (state.current_index !== null) {
        await playCurrentQueueItem();
      } else {
        expectedAudioSongIdRef.current = null;
        await stereodromeCore.audioStop();
        await nativeMediaControls.clear();
        setIsPlaying(false);
      }
    } finally {
      handlingEndedRef.current = false;
    }
  }, [applyQueueState, playCurrentQueueItem]);

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
      const [queueState, playbackState, audioSettings] = await Promise.all([
        stereodromeCore.getQueue(),
        stereodromeCore.getPlaybackState(),
        stereodromeCore.getAudioProcessingSettings(),
      ]);

      if (!mounted) {
        return;
      }

      await applyQueueState(queueState);
      await applyAudioProcessingSettings(audioSettings);
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
        (settings) => {
          void applyAudioProcessingSettings(settings).catch((playbackError) => {
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

          const queueSongId = currentSongRef.current?.id ?? null;
          const audioMovedToQueuedSong =
            status.current_song_id !== null &&
            queueSongId !== null &&
            status.current_song_id !== queueSongId &&
            queueRef.current.some((song) => song.id === status.current_song_id);

          updateAudioStatus(status);
          if (expectedAudioSongIdRef.current) {
            return;
          }

          if (audioMovedToQueuedSong && !crossfadeInProgressRef.current) {
            const state = await stereodromeCore.playNext(false);
            await applyQueueState(state);
            void prepareNextPlayback().catch(() => {});
            return;
          }

          if (status.current_song_id) {
            const now = Date.now();
            const previousReport = lastProgressReportRef.current;
            const shouldReport =
              !previousReport ||
              previousReport.songId !== status.current_song_id ||
              previousReport.isPlaying !== status.is_playing ||
              Math.abs(previousReport.position - status.position) >= 15 ||
              now - previousReport.at >= 15_000;

            if (shouldReport) {
              lastProgressReportRef.current = {
                at: now,
                isPlaying: status.is_playing,
                position: status.position,
                songId: status.current_song_id,
              };
              await stereodromeCore.reportPlaybackProgress({
                song_id: status.current_song_id,
                position_seconds: status.position,
                duration_seconds: status.duration,
                is_playing: status.is_playing,
              });
            }
          } else {
            lastProgressReportRef.current = null;
          }

          const settings = audioProcessingSettingsRef.current;
          const crossfadeWindowSeconds =
            (settings?.crossfade_duration_ms ?? 0) / 1000;
          const shouldCrossfade =
            settings?.crossfade_enabled === true &&
            status.is_playing &&
            status.current_song_id !== null &&
            crossfadeAttemptedSongIdRef.current !== status.current_song_id &&
            status.duration > 0 &&
            status.duration - status.position <= crossfadeWindowSeconds &&
            status.duration - status.position > 0.5 &&
            !crossfadeInProgressRef.current;
          if (shouldCrossfade) {
            crossfadeInProgressRef.current = true;
            crossfadeAttemptedSongIdRef.current = status.current_song_id;
            try {
              const state = await stereodromeCore.audioCrossfadeNext();
              if (state) {
                await applyQueueState(state);
                void prepareNextPlayback().catch(() => {});
              }
            } finally {
              crossfadeInProgressRef.current = false;
            }
          }

          const playbackFinished =
            status.current_song_id !== null &&
            status.duration > 0 &&
            status.position >= status.duration - 0.2 &&
            !status.is_playing;
          if (playbackFinished) {
            await handlePlaybackEnded();
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
  }, [
    applyQueueState,
    handlePlaybackEnded,
    isAppActive,
    prepareNextPlayback,
    updateAudioStatus,
  ]);

  const playSong = useCallback(
    async (song: PlayableSong, songs: PlayableSong[] = [song]) => {
      setError(null);
      setCurrentSong(song);

      try {
        await ensurePlayerReady();

        const state = await stereodromeCore.playSongWithQueue(
          song.id,
          songs.map((candidate) => candidate.id)
        );
        await applyQueueState(state);
        await playCurrentQueueItem();
      } catch (playbackError) {
        setError(errorMessage(playbackError));
      }
    },
    [applyQueueState, playCurrentQueueItem]
  );

  const toggle = useCallback(async () => {
    try {
      await ensurePlayerReady();
      if (isPlayingRef.current) {
        await stereodromeCore.audioPause();
        const status = await stereodromeCore.audioGetStatus();
        updateAudioStatus(status);
        await persistPlaybackPosition(false);
      } else if (currentIndexRef.current !== null) {
        const status = await stereodromeCore.audioGetStatus();
        if (status.current_song_id) {
          await stereodromeCore.audioResume();
          updateAudioStatus(await stereodromeCore.audioGetStatus());
        } else {
          await playCurrentQueueItem(
            restoredStartPositionRef.current ?? positionRef.current
          );
        }
      }
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [persistPlaybackPosition, playCurrentQueueItem, updateAudioStatus]);

  const toggleRepeat = useCallback(async () => {
    try {
      await ensurePlayerReady();
      const state = await stereodromeCore.cycleRepeatMode();
      await applyQueueState(state);
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [applyQueueState]);

  const rerollNext = useCallback(async () => {
    try {
      await ensurePlayerReady();
      const state = await stereodromeCore.rerollNext();
      await applyQueueState(state);
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [applyQueueState]);

  const toggleShuffle = useCallback(async () => {
    try {
      await ensurePlayerReady();
      const state = await stereodromeCore.toggleShuffle();
      await applyQueueState(state);
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [applyQueueState]);

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
    try {
      await ensurePlayerReady();
      const state = await stereodromeCore.playNext(true);
      await applyQueueState(state);
      await playCurrentQueueItem();
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [applyQueueState, playCurrentQueueItem]);

  const previous = useCallback(async () => {
    try {
      await ensurePlayerReady();
      const state = await stereodromeCore.playPrevious();
      await applyQueueState(state);
      await playCurrentQueueItem();
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [applyQueueState, playCurrentQueueItem]);

  const playQueueIndex = useCallback(
    async (index: number) => {
      try {
        const state = await stereodromeCore.playQueueItem(index);
        await applyQueueState(state);
        await playCurrentQueueItem();
      } catch (playbackError) {
        setError(errorMessage(playbackError));
      }
    },
    [applyQueueState, playCurrentQueueItem]
  );

  const removeQueueIndex = useCallback(
    async (index: number) => {
      try {
        const state = await stereodromeCore.removeFromQueue(index);
        await applyQueueState(state);
      } catch (playbackError) {
        setError(errorMessage(playbackError));
      }
    },
    [applyQueueState]
  );

  const clearQueue = useCallback(async () => {
    try {
      const state = await stereodromeCore.clearQueue();
      expectedAudioSongIdRef.current = null;
      restoredStartPositionRef.current = null;
      await stereodromeCore.audioStop();
      await applyQueueState(state);
      await nativeMediaControls.clear();
      positionRef.current = 0;
      durationRef.current = 0;
      setPosition(0);
      setDuration(0);
      setIsPlaying(false);
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [applyQueueState]);

  const nextIndex =
    currentIndex === null
      ? pendingNavigationIndex === null
        ? null
        : Math.min(pendingNavigationIndex, queue.length - 1)
      : currentIndex + 1 < queue.length
        ? currentIndex + 1
        : repeatMode === "All"
          ? 0
          : null;
  const nextSong = nextIndex === null ? null : (queue[nextIndex] ?? null);

  useEffect(() => {
    void nativeMediaControls
      .sync({
        currentSong,
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
