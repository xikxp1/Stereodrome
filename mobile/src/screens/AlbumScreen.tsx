import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function AlbumScreen({ albumId }: { albumId: string; title: string }) {
  const playback = usePlayback();
  const view = useViewStack();
  const songs = useQuery({
    queryKey: ["album-songs", albumId],
    queryFn: () => stereodromeCore.getSongs(albumId),
    enabled: !!albumId,
  });

  return (
    <SelectableList
      empty={songs.isLoading ? "Loading songs" : "No songs"}
      options={(songs.data ?? []).map((song) => ({
        label: song.title,
        sublabel: song.artist ?? undefined,
        onSelect: async () => {
          await playback.playSong(song, songs.data ?? [song]);
          view.showNowPlaying();
        },
      }))}
    />
  );
}
