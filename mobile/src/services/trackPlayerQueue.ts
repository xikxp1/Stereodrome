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

  const current =
    state.current_index === null ? null : state.items[state.current_index];
  if (current) {
    await TrackPlayer.add({
      id: current.song_id,
      url: await stereodromeCore.getStreamUri(current.song_id),
      contentType: "audio/mpeg",
      title: current.title,
      artist: current.artist,
      album: current.album,
      duration: current.duration,
      type: TrackType.Default,
    });
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
    case "One":
      return RepeatMode.Track;
    case "All":
    case "Off":
    default:
      return RepeatMode.Off;
  }
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}
