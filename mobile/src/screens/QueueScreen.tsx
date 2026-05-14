import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useViewStack } from "@/context/ViewContext";

export function QueueScreen() {
  const playback = usePlayback();
  const view = useViewStack();

  return (
    <SelectableList
      empty="Queue is empty"
      options={[
        ...(playback.queue.length > 0
          ? [
              {
                label: "Clear Queue",
                sublabel: `${playback.queue.length} songs`,
                onSelect: playback.clearQueue,
              },
            ]
          : []),
        ...playback.queue.map((song, index) => ({
          label:
            playback.currentSong?.id === song.id
              ? `Now: ${song.title}`
              : song.title,
          sublabel: [song.artist, song.album].filter(Boolean).join(" - "),
          onSelect: async () => {
            await playback.playQueueIndex(index);
            view.showNowPlaying();
          },
          onLongSelect: () => playback.removeQueueIndex(index),
        })),
      ]}
    />
  );
}
