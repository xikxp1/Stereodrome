import TrackPlayer, { RepeatMode, TrackType } from "react-native-track-player";

import { stereodromeCore } from "@/services/stereodromeCore";
import type { AudioProcessingSettings, QueueState } from "@/types/music";

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
      contentType: "audio/mpeg",
      title: song.title,
      artist: song.artist,
      album: song.album,
      duration: song.duration,
      type: TrackType.Default,
    }))
  );

  if (tracks.length > 0) {
    await TrackPlayer.add(tracks);
    if (state.current_index !== null) {
      await TrackPlayer.skip(state.current_index);
    }
  }
}

export async function applyAudioProcessingSettingsToTrackPlayer(
  settings: AudioProcessingSettings
) {
  const preampGain = settings.normalization_enabled
    ? Math.pow(10, settings.preamp_db / 20)
    : 1;
  await TrackPlayer.setVolume(clamp(preampGain, 0, 1));
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

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}
