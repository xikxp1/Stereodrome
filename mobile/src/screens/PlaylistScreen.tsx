import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function PlaylistScreen({
  playlistId,
}: {
  playlistId: string;
  title: string;
}) {
  const playback = usePlayback();
  const view = useViewStack();
  const songs = useQuery({
    queryKey: ["playlist-songs", playlistId],
    queryFn: () => stereodromeCore.getPlaylistSongs(playlistId),
    enabled: !!playlistId,
  });

  return (
    <SelectableList
      empty={songs.isLoading ? "Loading playlist" : "No songs"}
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
