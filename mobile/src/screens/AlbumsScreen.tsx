import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { coreClient } from "@/core/client";
import {
  useFileState,
  usePlaybackActions,
  useStereodrome,
} from "@/core/selectors";
import { useViewStack } from "@/context/ViewContext";
import { visibleAlbums, visibleSongs } from "@/services/offlineLibrary";

export function AlbumsScreen() {
  const view = useViewStack();
  const playback = usePlaybackActions();
  const stereodrome = useStereodrome();
  const fileState = useFileState();
  const albums = useQuery({
    queryKey: ["albums"],
    queryFn: () =>
      coreClient.dispatchTyped({ type: "get-albums", artist_id: null }),
  });
  const songs = useQuery({
    queryKey: ["songs"],
    queryFn: () =>
      coreClient.dispatchTyped({
        type: "get-songs",
        album_id: null,
        artist_id: null,
      }),
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
      await coreClient.dispatchTyped({
        type: "get-songs",
        album_id: albumId,
        artist_id: null,
      }),
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
