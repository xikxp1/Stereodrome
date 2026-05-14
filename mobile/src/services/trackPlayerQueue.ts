import TrackPlayer, { RepeatMode } from "react-native-track-player";

import { stereodromeCore } from "@/services/stereodromeCore";
import type { QueueState } from "@/types/music";

let suppressTrackEventsUntil = 0;

export function shouldSuppressTrackPlayerQueueEvent() {
  return Date.now() < suppressTrackEventsUntil;
}

export async function applyQueueStateToTrackPlayer(state: QueueState) {
  await TrackPlayer.setRepeatMode(repeatModeForTrackPlayer(state.repeat_mode));

  if (state.current_index === null && state.pending_navigation_index !== null) {
    return;
  }

  suppressTrackEventsUntil = Date.now() + 1_000;
  await TrackPlayer.reset();

  const tracks = await Promise.all(
    state.items.map(async (song) => ({
      id: song.song_id,
      url: await stereodromeCore.getStreamUri(song.song_id),
      title: song.title,
      artist: song.artist,
      album: song.album,
      duration: song.duration,
    }))
  );

  if (tracks.length > 0) {
    await TrackPlayer.add(tracks);
    if (state.current_index !== null) {
      await TrackPlayer.skip(state.current_index);
    }
  }
}

function repeatModeForTrackPlayer(mode: QueueState["repeat_mode"]) {
  switch (mode) {
    case "All":
      return RepeatMode.Queue;
    case "One":
      return RepeatMode.Track;
    case "Off":
    default:
      return RepeatMode.Off;
  }
}
