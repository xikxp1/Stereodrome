import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { visibleSongs } from "@/services/offlineLibrary";
import { stereodromeCore } from "@/services/stereodromeCore";

export function PlaylistScreen({
  playlistId,
}: {
  playlistId: string;
  title: string;
}) {
  const playback = usePlayback();
  const stereodrome = useStereodrome();
  const view = useViewStack();
  const songs = useQuery({
    queryKey: ["playlist-songs", playlistId],
    queryFn: () => stereodromeCore.getPlaylistSongs(playlistId),
    enabled: !!playlistId,
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
          ? "Loading playlist"
          : stereodrome.offlineMode
            ? "No offline playlist songs"
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
              await stereodromeCore.downloadPlaylist(playlistId);
              await stereodrome.refreshOfflineSongIds();
            },
      }))}
    />
  );
}
