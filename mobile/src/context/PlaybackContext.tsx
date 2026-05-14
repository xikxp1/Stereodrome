import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import TrackPlayer, {
  Capability,
  Event,
  State,
  usePlaybackState,
  useProgress,
} from "react-native-track-player";

import { stereodromeCore } from "@/services/stereodromeCore";
import {
  applyQueueStateToTrackPlayer,
  shouldSuppressTrackPlayerQueueEvent,
} from "@/services/trackPlayerQueue";
import type { PlayableSong, QueueItem, QueueState } from "@/types/music";

type PlaybackContextValue = {
  currentSong: PlayableSong | null;
  error: string | null;
  isPlaying: boolean;
  nextSong: PlayableSong | null;
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

let playerReadyPromise: Promise<void> | null = null;

async function ensurePlayerReady() {
  if (!playerReadyPromise) {
    playerReadyPromise = (async () => {
      try {
        await TrackPlayer.setupPlayer({
          autoHandleInterruptions: true,
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        if (!message.toLowerCase().includes("already")) {
          throw error;
        }
      }

      await TrackPlayer.updateOptions({
        capabilities: [
          Capability.Play,
          Capability.Pause,
          Capability.SkipToNext,
          Capability.SkipToPrevious,
          Capability.SeekTo,
        ],
        compactCapabilities: [
          Capability.Play,
          Capability.Pause,
          Capability.SkipToNext,
        ],
      });
    })();
  }

  try {
    await playerReadyPromise;
  } catch (error) {
    playerReadyPromise = null;
    throw error;
  }
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
  const playbackState = usePlaybackState();
  const progress = useProgress(5_000);
  const [error, setError] = useState<string | null>(null);
  const [currentSong, setCurrentSong] = useState<PlayableSong | null>(null);
  const [currentIndex, setCurrentIndex] = useState<number | null>(null);
  const [queue, setQueue] = useState<PlayableSong[]>([]);
  const [repeatMode, setRepeatMode] =
    useState<QueueState["repeat_mode"]>("Off");
  const [shuffleEnabled, setShuffleEnabled] = useState(false);
  const currentIndexRef = useRef<number | null>(null);
  const queueRef = useRef<PlayableSong[]>([]);

  const applyQueueState = useCallback(async (state: QueueState) => {
    const songs = state.items.map(playableFromQueueItem);
    const index = state.current_index;

    currentIndexRef.current = index;
    queueRef.current = songs;
    setQueue(songs);
    setCurrentIndex(index);
    setCurrentSong(index === null ? null : (songs[index] ?? null));
    setRepeatMode(state.repeat_mode);
    setShuffleEnabled(state.shuffle);

    await ensurePlayerReady();
    await applyQueueStateToTrackPlayer(state);
  }, []);

  useEffect(() => {
    let mounted = true;
    ensurePlayerReady()
      .then(() => {
        if (mounted) {
          setError(null);
        }
        return stereodromeCore.getQueue();
      })
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

    const errorSubscription = TrackPlayer.addEventListener(
      Event.PlaybackError,
      (playbackError) => {
        setError(playbackError.message);
      }
    );

    const trackSubscription = TrackPlayer.addEventListener(
      Event.PlaybackActiveTrackChanged,
      (event) => {
        const index = event.index ?? null;
        if (index === null) {
          currentIndexRef.current = null;
          setCurrentIndex(null);
          setCurrentSong(null);
          return;
        }

        if (shouldSuppressTrackPlayerQueueEvent()) {
          currentIndexRef.current = index;
          setCurrentIndex(index);
          setCurrentSong(queueRef.current[index] ?? null);
          return;
        }

        const previousIndex = currentIndexRef.current;
        const queueLength = queueRef.current.length;
        const movedForward =
          previousIndex !== null &&
          (index === previousIndex + 1 ||
            (previousIndex === queueLength - 1 && index === 0));

        void (async () => {
          const state = movedForward
            ? await stereodromeCore.playNext(false)
            : await stereodromeCore.playQueueItem(index);
          await applyQueueState(state);
          void stereodromeCore.prefetchNext().catch(() => {});
        })().catch((playbackError) => {
          setError(errorMessage(playbackError));
        });
      }
    );

    const unsubscribeQueue = stereodromeCore.addEventListener<QueueState>(
      "queue-changed",
      (state) => {
        void applyQueueState(state).catch((playbackError) => {
          setError(errorMessage(playbackError));
        });
      }
    );

    return () => {
      mounted = false;
      errorSubscription.remove();
      trackSubscription.remove();
      unsubscribeQueue();
    };
  }, [applyQueueState]);

  useEffect(() => {
    if (!currentSong) {
      return;
    }

    void stereodromeCore
      .reportPlaybackProgress({
        song_id: currentSong.id,
        position_seconds: progress.position,
        duration_seconds: progress.duration || currentSong.duration || 0,
        is_playing: playbackState.state === State.Playing,
      })
      .catch((playbackError) => {
        setError(errorMessage(playbackError));
      });
  }, [currentSong, playbackState.state, progress.duration, progress.position]);

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
        await TrackPlayer.play();
        void stereodromeCore.prefetchNext().catch(() => {});
      } catch (playbackError) {
        setError(errorMessage(playbackError));
      }
    },
    [applyQueueState]
  );

  const toggle = useCallback(async () => {
    try {
      await ensurePlayerReady();
      const state = await TrackPlayer.getPlaybackState();
      if (state.state === State.Playing) {
        await TrackPlayer.pause();
      } else {
        await TrackPlayer.play();
      }
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, []);

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
      if (!currentSong) {
        return;
      }

      try {
        await ensurePlayerReady();
        const progress = await TrackPlayer.getProgress();
        const duration = progress.duration || currentSong.duration || 0;
        const nextPosition = Math.max(
          0,
          duration > 0
            ? Math.min(duration, progress.position + seconds)
            : progress.position + seconds
        );
        await TrackPlayer.seekTo(nextPosition);
      } catch (playbackError) {
        setError(errorMessage(playbackError));
      }
    },
    [currentSong]
  );

  const next = useCallback(async () => {
    try {
      await ensurePlayerReady();
      const state = await stereodromeCore.playNext(true);
      await applyQueueState(state);
      await TrackPlayer.play();
      void stereodromeCore.prefetchNext().catch(() => {});
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [applyQueueState]);

  const previous = useCallback(async () => {
    try {
      await ensurePlayerReady();
      const state = await stereodromeCore.playPrevious();
      await applyQueueState(state);
      await TrackPlayer.play();
      void stereodromeCore.prefetchNext().catch(() => {});
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [applyQueueState]);

  const playQueueIndex = useCallback(
    async (index: number) => {
      try {
        const state = await stereodromeCore.playQueueItem(index);
        await applyQueueState(state);
        await TrackPlayer.play();
        void stereodromeCore.prefetchNext().catch(() => {});
      } catch (playbackError) {
        setError(errorMessage(playbackError));
      }
    },
    [applyQueueState]
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
      await applyQueueState(state);
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [applyQueueState]);

  const isPlaying = playbackState.state === State.Playing;
  const nextIndex =
    currentIndex === null
      ? null
      : currentIndex + 1 < queue.length
        ? currentIndex + 1
        : repeatMode === "All"
          ? 0
          : null;
  const nextSong = nextIndex === null ? null : (queue[nextIndex] ?? null);
  const value = useMemo(
    () => ({
      currentSong,
      error,
      isPlaying,
      nextSong,
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
      error,
      isPlaying,
      next,
      nextSong,
      playSong,
      playQueueIndex,
      previous,
      queue,
      clearQueue,
      removeQueueIndex,
      repeatMode,
      rerollNext,
      seekBy,
      shuffleEnabled,
      toggle,
      toggleRepeat,
      toggleShuffle,
    ]
  );

  return (
    <PlaybackContext.Provider value={value}>
      {children}
    </PlaybackContext.Provider>
  );
}

export function usePlayback() {
  const value = useContext(PlaybackContext);
  if (!value) {
    throw new Error("usePlayback must be used inside PlaybackProvider");
  }
  return value;
}
