import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import {
  useFileState,
  usePlaybackActions,
  useStereodrome,
} from "@/core/selectors";
import { useViewStack } from "@/context/ViewContext";
import { visibleSongs } from "@/services/offlineLibrary";
import { stereodromeCore } from "@/services/stereodromeCore";

export function PlaylistsScreen() {
  const view = useViewStack();
  const playback = usePlaybackActions();
  const stereodrome = useStereodrome();
  const fileState = useFileState();
  const playlists = useQuery({
    queryKey: ["playlists", stereodrome.offlineMode ? "offline" : "online"],
    queryFn: stereodromeCore.getPlaylists,
  });

  async function playPlaylist(playlistId: string) {
    const songs = await stereodromeCore.getPlaylistSongs(playlistId);
    const playableSongs = visibleSongs(
      songs,
      stereodrome.offlineMode,
      fileState.offlineSongIds
    );
    const firstSong = playableSongs[0];
    if (firstSong) {
      await playback.playSong(firstSong, playableSongs);
      view.showNowPlaying();
    }
  }

  const shownPlaylists = (playlists.data ?? []).filter(
    (playlist) => !stereodrome.offlineMode || playlist.saved_offline
  );

  return (
    <SelectableList
      empty={
        playlists.isLoading
          ? "Loading playlists"
          : stereodrome.offlineMode
            ? "No offline playlists"
            : "No playlists"
      }
      options={shownPlaylists.map((playlist) => ({
        label: playlist.name,
        sublabel: playlist.saved_offline
          ? `${playlist.song_count} songs • saved offline`
          : `${playlist.song_count} songs`,
        onSelect: () => {
          view.push({
            name: "playlist",
            title: playlist.name,
            params: { playlistId: playlist.id, title: playlist.name },
          });
        },
        onLongSelect: () => playPlaylist(playlist.id),
      }))}
    />
  );
}
