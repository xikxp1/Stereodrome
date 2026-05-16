import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { visibleSongs } from "@/services/offlineLibrary";
import { stereodromeCore } from "@/services/stereodromeCore";

export function AlbumScreen({ albumId }: { albumId: string; title: string }) {
  const playback = usePlayback();
  const stereodrome = useStereodrome();
  const view = useViewStack();
  const songs = useQuery({
    queryKey: ["album-songs", albumId],
    queryFn: () => stereodromeCore.getSongs(albumId),
    enabled: !!albumId,
  });
  const shownSongs = visibleSongs(
    songs.data ?? [],
    stereodrome.offlineMode,
    stereodrome.offlineSongIds
  );

  return (
    <SelectableList
      empty={
        songs.isLoading
          ? "Loading songs"
          : stereodrome.offlineMode
            ? "No offline songs"
            : "No songs"
      }
      options={shownSongs.map((song) => ({
        label: song.title,
        sublabel: song.artist ?? undefined,
        onSelect: async () => {
          await playback.playSong(
            song,
            shownSongs.length ? shownSongs : [song]
          );
          view.showNowPlaying();
        },
        onLongSelect: stereodrome.offlineMode
          ? undefined
          : async () => {
              await stereodromeCore.downloadAlbum(albumId);
              await stereodrome.refreshOfflineSongIds();
            },
      }))}
    />
  );
}
