import { stereodromeCore } from "@/services/stereodromeCore";
import type { PlayableSong } from "@/types/music";

type NativeMediaControlsSnapshot = {
  canPlay: boolean;
  currentSong: PlayableSong | null;
  duration: number;
  isPlaying: boolean;
  position: number;
  queue: PlayableSong[];
  currentIndex: number | null;
  nextSong: PlayableSong | null;
};

class NativeMediaControlsSync {
  private artworkCache = new Map<string, string | null>();
  private lastInfoKey: string | null = null;
  private lastProgressKey: string | null = null;
  private lastProgressPosition: number | null = null;
  private isCleared = false;
  private syncGeneration = 0;

  addInvalidatedListener(listener: () => void): () => void {
    return stereodromeCore.addNativePlaybackInvalidatedListener(listener);
  }

  async sync(snapshot: NativeMediaControlsSnapshot): Promise<void> {
    const generation = ++this.syncGeneration;
    const { currentSong } = snapshot;

    if (!currentSong) {
      await this.clear();
      return;
    }

    const artworkUri = await this.getArtworkUri(currentSong.id);
    if (generation !== this.syncGeneration) {
      return;
    }

    const duration = snapshot.duration || currentSong.duration || 0;
    const queueIndex = snapshot.currentIndex;
    const queueCount = snapshot.queue.length;
    const canPrevious = queueCount > 1 && queueIndex !== null;
    const canNext = snapshot.nextSong !== null;
    const canSeek = duration > 0;
    const canPlay = snapshot.canPlay;
    const infoKey = JSON.stringify({
      song_id: currentSong.id,
      title: currentSong.title,
      artist: currentSong.artist ?? null,
      album: currentSong.album ?? null,
      duration,
      artworkUri,
      queueIndex,
      queueCount,
      canNext,
      canPlay,
      canPrevious,
      canSeek,
    });

    if (infoKey !== this.lastInfoKey) {
      await stereodromeCore.setNowPlayingInfo({
        song_id: currentSong.id,
        title: currentSong.title,
        artist: currentSong.artist ?? null,
        album: currentSong.album ?? null,
        duration_seconds: duration,
        position_seconds: snapshot.position,
        is_playing: snapshot.isPlaying,
        artwork_uri: artworkUri,
        queue_index: queueIndex,
        queue_count: queueCount,
        can_next: canNext,
        can_play: canPlay,
        can_previous: canPrevious,
        can_seek: canSeek,
      });
      this.lastInfoKey = infoKey;
      this.lastProgressKey = null;
      this.lastProgressPosition = null;
      this.isCleared = false;
    }

    const roundedPosition = Math.max(0, Number(snapshot.position.toFixed(2)));
    const progressKey = JSON.stringify({
      song_id: currentSong.id,
      duration,
      isPlaying: snapshot.isPlaying,
    });
    const progressMovedEnough =
      this.lastProgressPosition === null ||
      Math.abs(roundedPosition - this.lastProgressPosition) >=
        (snapshot.isPlaying ? 5 : 0.01);

    if (progressKey !== this.lastProgressKey || progressMovedEnough) {
      await stereodromeCore.updateNowPlayingProgress({
        song_id: currentSong.id,
        duration_seconds: duration,
        position_seconds: roundedPosition,
        is_playing: snapshot.isPlaying,
      });
      this.lastProgressKey = progressKey;
      this.lastProgressPosition = roundedPosition;
    }
  }

  async clear(): Promise<void> {
    this.syncGeneration += 1;
    if (this.isCleared) {
      return;
    }
    this.lastInfoKey = null;
    this.lastProgressKey = null;
    this.lastProgressPosition = null;
    this.isCleared = true;
    await stereodromeCore.clearNowPlayingInfo();
  }

  private async getArtworkUri(songId: string): Promise<string | null> {
    if (this.artworkCache.has(songId)) {
      return this.artworkCache.get(songId) ?? null;
    }

    try {
      const artworkUri = await stereodromeCore.getSongCoverArtUri(songId, 512);
      this.artworkCache.set(songId, artworkUri);
      return artworkUri;
    } catch {
      this.artworkCache.set(songId, null);
      return null;
    }
  }
}

export const nativeMediaControls = new NativeMediaControlsSync();
