import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import {
  useFileState,
  usePlaybackActions,
  useStereodrome,
} from "@/core/selectors";
import { useViewStack } from "@/context/ViewContext";
import { visibleAlbums, visibleSongs } from "@/services/offlineLibrary";
import { stereodromeCore } from "@/services/stereodromeCore";

export function ArtistScreen({
  artistId,
  title,
}: {
  artistId: string;
  title: string;
}) {
  const view = useViewStack();
  const playback = usePlaybackActions();
  const stereodrome = useStereodrome();
  const fileState = useFileState();
  const albums = useQuery({
    queryKey: ["artist-albums", artistId],
    queryFn: () => stereodromeCore.getAlbums(artistId),
    enabled: Boolean(artistId),
  });
  const songs = useQuery({
    queryKey: ["artist-songs", artistId],
    queryFn: () => stereodromeCore.getSongs(undefined, artistId),
    enabled: Boolean(artistId) && stereodrome.offlineMode,
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

  async function playAllSongs() {
    const artistSongs = visibleSongs(
      await stereodromeCore.getSongs(undefined, artistId),
      stereodrome.offlineMode,
      fileState.offlineSongIds
    );
    const firstSong = artistSongs[0];
    if (firstSong) {
      await playback.playSong(firstSong, artistSongs);
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
            : "No albums"
      }
      options={[
        {
          label: "All Songs",
          onSelect: () => {
            view.push({
              name: "songs",
              title,
              params: { artistId },
            });
          },
          onLongSelect: playAllSongs,
        },
        ...shownAlbums.map((album) => ({
          label: album.name,
          ...(album.year !== null && album.year !== 0
            ? { sublabel: String(album.year) }
            : {}),
          onSelect: () => {
            view.push({
              name: "album",
              title: album.name,
              params: { albumId: album.id, title: album.name },
            });
          },
          onLongSelect: () => playAlbum(album.id),
        })),
      ]}
    />
  );
}
