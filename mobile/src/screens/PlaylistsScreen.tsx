import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function PlaylistsScreen() {
  const view = useViewStack();
  const playback = usePlayback();
  const playlists = useQuery({
    queryKey: ["playlists"],
    queryFn: stereodromeCore.getPlaylists,
  });

  async function playPlaylist(playlistId: string) {
    const songs = await stereodromeCore.getPlaylistSongs(playlistId);
    if (songs.length > 0) {
      await playback.playSong(songs[0], songs);
      view.showNowPlaying();
    }
  }

  return (
    <SelectableList
      empty={playlists.isLoading ? "Loading playlists" : "No playlists"}
      options={(playlists.data ?? []).map((playlist) => ({
        label: playlist.name,
        sublabel: `${playlist.song_count} songs`,
        onSelect: () =>
          view.push({
            name: "playlist",
            title: playlist.name,
            params: { playlistId: playlist.id, title: playlist.name },
          }),
        onLongSelect: () => playPlaylist(playlist.id),
      }))}
    />
  );
}
