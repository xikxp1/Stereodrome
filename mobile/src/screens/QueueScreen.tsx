import { SelectableList } from "@/components/SelectableList";
import { useProtectedSelectableAction } from "@/components/protectedSelectableAction";
import { usePlayback } from "@/context/PlaybackContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { songFileState } from "@/services/offlineLibrary";

export function QueueScreen() {
  const playback = usePlayback();
  const stereodrome = useStereodrome();
  const view = useViewStack();
  const { protectedActionRows } = useProtectedSelectableAction(
    `queue:${playback.queue.length}:${playback.currentSong?.id ?? ""}`
  );

  return (
    <SelectableList
      empty="Queue is empty"
      options={[
        ...(playback.queue.length > 0
          ? protectedActionRows({
              id: "clear-queue",
              label: "Clear Queue",
              sublabel: `${playback.queue.length} songs`,
              confirmLabel: "Confirm Clear",
              confirmSublabel: "Use wheel select to clear queue",
              cancelLabel: "Cancel Clear",
              cancelSublabel: "Keep queue",
              onConfirm: playback.clearQueue,
            })
          : []),
        ...playback.queue.map((song, index) => ({
          fileState: songFileState(
            song.id,
            stereodrome.offlineSongIds,
            stereodrome.downloadingSongIds
          ),
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
