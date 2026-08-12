import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { coreClient } from "@/core/client";
import {
  useFileState,
  usePlaybackActions,
  useStereodrome,
} from "@/core/selectors";
import { useViewStack } from "@/context/ViewContext";
import { visibleArtists, visibleSongs } from "@/services/offlineLibrary";

export function ArtistsScreen() {
  const view = useViewStack();
  const playback = usePlaybackActions();
  const stereodrome = useStereodrome();
  const fileState = useFileState();
  const artists = useQuery({
    queryKey: ["artists"],
    queryFn: () => coreClient.dispatchTyped({ type: "get-artists" }),
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
  const shownArtists = visibleArtists(
    artists.data ?? [],
    songs.data ?? [],
    stereodrome.offlineMode,
    fileState.offlineSongIds
  );
  const isLoading =
    artists.isLoading || (stereodrome.offlineMode && songs.isLoading);

  async function playArtist(artistId: string) {
    const artistSongs = visibleSongs(
      await coreClient.dispatchTyped({
        type: "get-songs",
        album_id: null,
        artist_id: artistId,
      }),
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
          ? "Loading artists"
          : stereodrome.offlineMode
            ? "No offline artists"
            : "No artists synced"
      }
      options={shownArtists.map((artist) => ({
        label: artist.name,
        sublabel: `${artist.album_count} albums`,
        onSelect: () => {
          view.push({
            name: "artist",
            title: artist.name,
            params: { artistId: artist.id, title: artist.name },
          });
        },
        onLongSelect: () => playArtist(artist.id),
      }))}
    />
  );
}
