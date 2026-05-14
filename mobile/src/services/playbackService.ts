import TrackPlayer, { Event } from "react-native-track-player";

import { stereodromeCore } from "@/services/stereodromeCore";
import { applyQueueStateToTrackPlayer } from "@/services/trackPlayerQueue";

export async function playbackService() {
  TrackPlayer.addEventListener(Event.RemotePlay, () => TrackPlayer.play());
  TrackPlayer.addEventListener(Event.RemotePause, () => TrackPlayer.pause());
  TrackPlayer.addEventListener(Event.RemoteNext, async () => {
    const state = await stereodromeCore.playNext(true);
    await applyQueueStateToTrackPlayer(state);
    await TrackPlayer.play();
    void stereodromeCore.prefetchNext().catch(() => {});
  });
  TrackPlayer.addEventListener(Event.RemotePrevious, async () => {
    const state = await stereodromeCore.playPrevious();
    await applyQueueStateToTrackPlayer(state);
    await TrackPlayer.play();
    void stereodromeCore.prefetchNext().catch(() => {});
  });
  TrackPlayer.addEventListener(Event.RemoteSeek, (event) =>
    TrackPlayer.seekTo(event.position)
  );
}
