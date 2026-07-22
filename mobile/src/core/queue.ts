import type { PlayableSong, QueueItem } from "@/types/music";

export function queueItemFromSong(song: PlayableSong): QueueItem {
  return {
    song_id: song.id,
    title: song.title,
    artist: song.artist ?? "Unknown Artist",
    album: song.album ?? "Unknown Album",
    duration: song.duration ?? 0,
  };
}
