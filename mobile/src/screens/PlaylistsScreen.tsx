import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { visibleSongs } from "@/services/offlineLibrary";
import { stereodromeCore } from "@/services/stereodromeCore";

export function PlaylistsScreen() {
  const view = useViewStack();
  const playback = usePlayback();
  const stereodrome = useStereodrome();
  const playlists = useQuery({
    queryKey: ["playlists", stereodrome.offlineMode ? "offline" : "online"],
    queryFn: stereodromeCore.getPlaylists,
  });

  async function playPlaylist(playlistId: string) {
    const songs = await stereodromeCore.getPlaylistSongs(playlistId);
    const playableSongs = visibleSongs(
      songs,
      stereodrome.offlineMode,
      stereodrome.offlineSongIds
    );
    if (playableSongs.length > 0) {
      await playback.playSong(playableSongs[0], playableSongs);
      view.showNowPlaying();
    }
  }

  return (
    <SelectableList
      empty={
        playlists.isLoading
          ? "Loading playlists"
          : stereodrome.offlineMode
            ? "No offline playlists"
            : "No playlists"
      }
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
