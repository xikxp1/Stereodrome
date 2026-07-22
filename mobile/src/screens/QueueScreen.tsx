import { SelectableList } from "@/components/SelectableList";
import { useProtectedSelectableAction } from "@/components/protectedSelectableAction";
import {
  useFileState,
  usePlaybackActions,
  usePlaybackMetadata,
} from "@/core/selectors";
import { useViewStack } from "@/context/ViewContext";
import { songFileState } from "@/services/offlineLibrary";

export function QueueScreen() {
  const playback = usePlaybackMetadata();
  const playbackActions = usePlaybackActions();
  const fileState = useFileState();
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
              onConfirm: playbackActions.clearQueue,
            })
          : []),
        ...playback.queue.map((song, index) => ({
          fileState: songFileState(
            song.id,
            fileState.offlineSongIds,
            fileState.downloadingSongIds
          ),
          label:
            playback.currentSong?.id === song.id
              ? `Now: ${song.title}`
              : song.title,
          sublabel: [song.artist, song.album].filter(Boolean).join(" - "),
          onSelect: async () => {
            await playbackActions.playQueueIndex(index);
            view.showNowPlaying();
          },
          onLongSelect: () => playbackActions.removeQueueIndex(index),
        })),
      ]}
    />
  );
}
