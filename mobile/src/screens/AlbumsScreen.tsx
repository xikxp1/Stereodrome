import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlaybackActions } from "@/context/PlaybackContext";
import { useFileState, useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { visibleAlbums, visibleSongs } from "@/services/offlineLibrary";
import { stereodromeCore } from "@/services/stereodromeCore";

export function AlbumsScreen() {
  const view = useViewStack();
  const playback = usePlaybackActions();
  const stereodrome = useStereodrome();
  const fileState = useFileState();
  const albums = useQuery({
    queryKey: ["albums"],
    queryFn: () => stereodromeCore.getAlbums(),
  });
  const songs = useQuery({
    queryKey: ["songs"],
    queryFn: () => stereodromeCore.getSongs(),
    enabled: stereodrome.offlineMode,
  });
  const shownAlbums = visibleAlbums(
    albums.data ?? [],
    songs.data ?? [],
    stereodrome.offlineMode,
    fileState.offlineSongIds
  );
  const isLoading =
    albums.isLoading || (stereodrome.offlineMode && songs.isLoading);

  async function playAlbum(albumId: string) {
    const albumSongs = visibleSongs(
      await stereodromeCore.getSongs(albumId),
      stereodrome.offlineMode,
      fileState.offlineSongIds
    );
    const firstSong = albumSongs[0];
    if (firstSong) {
      await playback.playSong(firstSong, albumSongs);
      view.showNowPlaying();
    }
  }

  return (
    <SelectableList
      empty={
        isLoading
          ? "Loading albums"
          : stereodrome.offlineMode
            ? "No offline albums"
            : "No albums synced"
      }
      options={shownAlbums.map((album) => ({
        label: album.name,
        ...(album.artist_name == null ? {} : { sublabel: album.artist_name }),
        onSelect: () => {
          view.push({
            name: "album",
            title: album.name,
            params: { albumId: album.id, title: album.name },
          });
        },
        onLongSelect: () => playAlbum(album.id),
      }))}
    />
  );
}
