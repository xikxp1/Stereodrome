import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
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
    enabled: !!playlistId && !stereodrome.offlineMode,
  });

  return (
    <SelectableList
      empty={
        stereodrome.offlineMode
          ? "Playlists unavailable offline"
          : songs.isLoading
            ? "Loading playlist"
            : "No songs"
      }
      options={(songs.data ?? []).map((song) => ({
        label: song.title,
        sublabel: song.artist ?? undefined,
        onSelect: async () => {
          await playback.playSong(song, songs.data ?? [song]);
          view.showNowPlaying();
        },
        onLongSelect: async () => {
          await stereodromeCore.downloadPlaylist(playlistId);
          await stereodrome.refreshOfflineSongIds();
        },
      }))}
    />
  );
}
