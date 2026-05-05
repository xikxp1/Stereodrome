import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import TrackPlayer, {
  Capability,
  Event,
  RepeatMode,
  State,
  usePlaybackState,
} from "react-native-track-player";

import { stereodromeCore } from "@/services/stereodromeCore";
import type { PlayableSong } from "@/types/music";

type PlaybackContextValue = {
  currentSong: PlayableSong | null;
  error: string | null;
  isPlaying: boolean;
  nextSong: PlayableSong | null;
  repeatEnabled: boolean;
  shuffleEnabled: boolean;
  playSong(song: PlayableSong, queue?: PlayableSong[]): Promise<void>;
  toggle(): Promise<void>;
  toggleRepeat(): Promise<void>;
  toggleShuffle(): Promise<void>;
  rerollNext(): Promise<void>;
  seekBy(seconds: number): Promise<void>;
  next(): Promise<void>;
  previous(): Promise<void>;
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

function shuffled<T>(items: T[]): T[] {
  const result = [...items];
  for (let index = result.length - 1; index > 0; index -= 1) {
    const swapIndex = Math.floor(Math.random() * (index + 1));
    [result[index], result[swapIndex]] = [result[swapIndex], result[index]];
  }
  return result;
}

function moveItem<T>(items: T[], fromIndex: number, toIndex: number): T[] {
  const result = [...items];
  const [item] = result.splice(fromIndex, 1);
  if (item !== undefined) {
    result.splice(toIndex, 0, item);
  }
  return result;
}

function swapItems<T>(
  items: T[],
  firstIndex: number,
  secondIndex: number
): T[] {
  const result = [...items];
  [result[firstIndex], result[secondIndex]] = [
    result[secondIndex],
    result[firstIndex],
  ];
  return result;
}

function nextQueueIndex(
  queueLength: number,
  index: number | null,
  repeat: boolean
): number | null {
  if (queueLength === 0 || index === null) {
    return null;
  }

  if (index + 1 < queueLength) {
    return index + 1;
  }

  return repeat ? 0 : null;
}

async function swapNativeQueueItems(firstIndex: number, secondIndex: number) {
  if (firstIndex === secondIndex) {
    return;
  }

  if (firstIndex < secondIndex) {
    await TrackPlayer.move(firstIndex, secondIndex);
    await TrackPlayer.move(secondIndex - 1, firstIndex);
    return;
  }

  await TrackPlayer.move(firstIndex, secondIndex);
  await TrackPlayer.move(secondIndex + 1, firstIndex);
}

export function PlaybackProvider({ children }: { children: React.ReactNode }) {
  const playbackState = usePlaybackState();
  const [error, setError] = useState<string | null>(null);
  const [currentSong, setCurrentSong] = useState<PlayableSong | null>(null);
  const [currentIndex, setCurrentIndex] = useState<number | null>(null);
  const [queue, setQueue] = useState<PlayableSong[]>([]);
  const [repeatEnabled, setRepeatEnabled] = useState(false);
  const [shuffleEnabled, setShuffleEnabled] = useState(false);

  useEffect(() => {
    let mounted = true;
    ensurePlayerReady()
      .then(() => {
        if (mounted) {
          setError(null);
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
        setCurrentIndex(index);
        setCurrentSong(index === null ? null : (queue[index] ?? null));
      }
    );

    return () => {
      mounted = false;
      errorSubscription.remove();
      trackSubscription.remove();
    };
  }, [queue]);

  const playSong = useCallback(
    async (song: PlayableSong, songs: PlayableSong[] = [song]) => {
      setError(null);
      setCurrentSong(song);

      try {
        await ensurePlayerReady();

        const startIndex = Math.max(
          0,
          songs.findIndex((candidate) => candidate.id === song.id)
        );
        const orderedSongs = shuffleEnabled
          ? [
              ...songs.slice(0, startIndex + 1),
              ...shuffled(songs.slice(startIndex + 1)),
            ]
          : songs;

        setQueue(orderedSongs);
        setCurrentIndex(startIndex);
        const tracks = await Promise.all(
          orderedSongs.map(async (candidate) => ({
            id: candidate.id,
            url: await stereodromeCore.getStreamUri(candidate.id),
            title: candidate.title,
            artist: candidate.artist ?? undefined,
            album: candidate.album ?? undefined,
            duration: candidate.duration ?? undefined,
          }))
        );
        await TrackPlayer.reset();
        await TrackPlayer.add(tracks);
        await TrackPlayer.skip(startIndex);
        await TrackPlayer.play();
      } catch (playbackError) {
        setError(errorMessage(playbackError));
      }
    },
    [shuffleEnabled]
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
      const nextRepeatEnabled = !repeatEnabled;
      await TrackPlayer.setRepeatMode(
        nextRepeatEnabled ? RepeatMode.Queue : RepeatMode.Off
      );
      setRepeatEnabled(nextRepeatEnabled);
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [repeatEnabled]);

  const rerollNext = useCallback(async () => {
    const nextIndex = nextQueueIndex(queue.length, currentIndex, repeatEnabled);
    if (currentIndex === null || nextIndex === null) {
      return;
    }

    const candidates = queue
      .map((_, index) => index)
      .filter((index) => index !== currentIndex && index !== nextIndex);
    if (candidates.length === 0) {
      return;
    }

    try {
      await ensurePlayerReady();
      const randomIndex =
        candidates[Math.floor(Math.random() * candidates.length)];
      await swapNativeQueueItems(nextIndex, randomIndex);
      setQueue((currentQueue) =>
        swapItems(currentQueue, nextIndex, randomIndex)
      );
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [currentIndex, queue, repeatEnabled]);

  const toggleShuffle = useCallback(async () => {
    const nextShuffleEnabled = !shuffleEnabled;

    try {
      await ensurePlayerReady();
      if (nextShuffleEnabled && currentIndex !== null) {
        const firstUpcomingIndex = currentIndex + 1;
        const upcoming = queue.slice(firstUpcomingIndex);
        const nextUpcoming = shuffled(upcoming);
        let workingQueue = [...queue];

        for (let offset = 0; offset < nextUpcoming.length; offset += 1) {
          const targetIndex = firstUpcomingIndex + offset;
          const sourceIndex = workingQueue.findIndex(
            (candidate, index) =>
              index >= firstUpcomingIndex &&
              candidate.id === nextUpcoming[offset]?.id
          );

          if (sourceIndex !== -1 && sourceIndex !== targetIndex) {
            await TrackPlayer.move(sourceIndex, targetIndex);
            workingQueue = moveItem(workingQueue, sourceIndex, targetIndex);
          }
        }

        setQueue(workingQueue);
      }

      setShuffleEnabled(nextShuffleEnabled);
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [currentIndex, queue, shuffleEnabled]);

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
      await TrackPlayer.skipToNext();
      const index = await TrackPlayer.getActiveTrackIndex();
      if (index !== undefined) {
        setCurrentIndex(index);
        setCurrentSong(queue[index] ?? null);
      }
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [queue]);

  const previous = useCallback(async () => {
    try {
      await ensurePlayerReady();
      await TrackPlayer.skipToPrevious();
      const index = await TrackPlayer.getActiveTrackIndex();
      if (index !== undefined) {
        setCurrentIndex(index);
        setCurrentSong(queue[index] ?? null);
      }
    } catch (playbackError) {
      setError(errorMessage(playbackError));
    }
  }, [queue]);

  const isPlaying = playbackState.state === State.Playing;
  const nextIndex = nextQueueIndex(queue.length, currentIndex, repeatEnabled);
  const nextSong = nextIndex === null ? null : (queue[nextIndex] ?? null);
  const value = useMemo(
    () => ({
      currentSong,
      error,
      isPlaying,
      nextSong,
      repeatEnabled,
      shuffleEnabled,
      playSong,
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
      previous,
      repeatEnabled,
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
