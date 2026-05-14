import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { stereodromeCore } from "@/services/stereodromeCore";
import type {
  AudioPlaybackStatus,
  AudioProcessingSettings,
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
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const currentIndexRef = useRef<number | null>(null);
  const pendingNavigationIndexRef = useRef<number | null>(null);
  const currentSongRef = useRef<PlayableSong | null>(null);
  const queueRef = useRef<PlayableSong[]>([]);
  const isPlayingRef = useRef(false);
  const handlingEndedRef = useRef(false);
  const expectedAudioSongIdRef = useRef<string | null>(null);
  const audioProcessingSettingsRef = useRef<AudioProcessingSettings | null>(
    null
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
      setPosition(status.position);
      setDuration(status.duration);

      if (status.current_song_id) {
        const song =
          songs.find((candidate) => candidate.id === status.current_song_id) ??
          currentSongRef.current;
        currentSongRef.current = song;
        setCurrentSong(song);
      } else if (currentIndexRef.current === null) {
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

  const playCurrentQueueItem = useCallback(async () => {
    expectedAudioSongIdRef.current = currentSongRef.current?.id ?? null;
    const status = await stereodromeCore.audioPlayCurrent();
    updateAudioStatus(status);
    void stereodromeCore.prefetchNext().catch(() => {});
  }, [updateAudioStatus]);

  const applyAudioProcessingSettings = useCallback(
    async (settings: AudioProcessingSettings) => {
      audioProcessingSettingsRef.current = settings;
      await ensurePlayerReady();
      const status = await stereodromeCore.audioApplySettings();
      updateAudioStatus(status);
    },
    [updateAudioStatus]
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
        setIsPlaying(false);
      }
    } finally {
      handlingEndedRef.current = false;
    }
  }, [applyQueueState, playCurrentQueueItem]);

  useEffect(() => {
    let mounted = true;
    ensurePlayerReady()
      .then(() => stereodromeCore.getQueue())
      .then((state) => {
        if (mounted) {
          void applyQueueState(state);
        }
      })
      .catch((setupError) => {
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

    stereodromeCore
      .getAudioProcessingSettings()
      .then((settings) => {
        if (mounted) {
          void applyAudioProcessingSettings(settings);
        }
      })
      .catch((playbackError) => {
        if (mounted) {
          setError(errorMessage(playbackError));
        }
      });

    return () => {
      mounted = false;
      unsubscribeQueue();
      unsubscribeAudioSettings();
    };
  }, [applyAudioProcessingSettings, applyQueueState]);

  useEffect(() => {
    const interval = setInterval(() => {
      void stereodromeCore
        .audioGetStatus()
        .then(async (status) => {
          updateAudioStatus(status);
          if (expectedAudioSongIdRef.current) {
            return;
          }

          if (status.current_song_id) {
            await stereodromeCore.reportPlaybackProgress({
              song_id: status.current_song_id,
              position_seconds: status.position,
              duration_seconds: status.duration,
              is_playing: status.is_playing,
            });
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
          setError(errorMessage(playbackError));
        });
    }, 1_000);

    return () => {
      clearInterval(interval);
    };
  }, [handlePlaybackEnded, updateAudioStatus]);

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
        isPlayingRef.current = false;
        setIsPlaying(false);
      } else if (currentSongRef.current) {
        await stereodromeCore.audioResume();
        isPlayingRef.current = true;
        setIsPlaying(true);
      } else if (currentIndexRef.current !== null) {
        await playCurrentQueueItem();
      }
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [playCurrentQueueItem]);

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

  const seekBy = useCallback(async (seconds: number) => {
    try {
      await ensurePlayerReady();
      const status = await stereodromeCore.audioGetStatus();
      const nextPosition = Math.max(
        0,
        status.duration > 0
          ? Math.min(status.duration, status.position + seconds)
          : status.position + seconds
      );
      await stereodromeCore.audioSeek(nextPosition);
      setPosition(nextPosition);
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, []);

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
      await stereodromeCore.audioStop();
      await applyQueueState(state);
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
