import { useQuery, useQueryClient } from "@tanstack/react-query";

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
  const queryClient = useQueryClient();
  const songs = useQuery({
    queryKey: ["playlist-songs", playlistId],
    queryFn: () => stereodromeCore.getPlaylistSongs(playlistId),
    enabled: !!playlistId,
  });
  const playlists = useQuery({
    queryKey: ["playlists", stereodrome.offlineMode ? "offline" : "online"],
    queryFn: stereodromeCore.getPlaylists,
  });
  const playlist = (playlists.data ?? []).find(
    (item) => item.id === playlistId
  );
  const savedOffline = playlist?.saved_offline ?? false;
  const shownSongs = visibleSongs(
    songs.data ?? [],
    stereodrome.offlineMode,
    stereodrome.offlineSongIds
  );

  async function toggleSavedOffline() {
    if (!playlistId || stereodrome.offlineMode) {
      return;
    }
    await stereodromeCore.setPlaylistSavedOffline(playlistId, !savedOffline);
    await queryClient.invalidateQueries({ queryKey: ["playlists"] });
    await stereodrome.refreshOfflineSongIds();
  }

  const options = [
    ...(stereodrome.offlineMode
      ? []
      : [
          {
            label: savedOffline ? "Remove Offline Save" : "Save Offline",
            sublabel: savedOffline
              ? "Playlist songs stay cached until removed"
              : "Download and preserve playlist songs",
            onSelect: toggleSavedOffline,
          },
        ]),
    ...shownSongs.map((song) => ({
      label: song.title,
      sublabel: song.artist ?? undefined,
      onSelect: async () => {
        await playback.playSong(song, shownSongs.length ? shownSongs : [song]);
        view.showNowPlaying();
      },
      onLongSelect: stereodrome.offlineMode ? undefined : toggleSavedOffline,
    })),
  ];

  return (
    <SelectableList
      empty={
        songs.isLoading
          ? "Loading playlist"
          : stereodrome.offlineMode
            ? "No offline playlist songs"
            : "No songs"
      }
      options={options}
    />
  );
}
