import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { stereodromeCore } from "@/services/stereodromeCore";
import { useViewStack } from "@/context/ViewContext";
import { visibleArtists, visibleSongs } from "@/services/offlineLibrary";

export function ArtistsScreen() {
  const view = useViewStack();
  const playback = usePlayback();
  const stereodrome = useStereodrome();
  const artists = useQuery({
    queryKey: ["artists"],
    queryFn: stereodromeCore.getArtists,
  });
  const songs = useQuery({
    queryKey: ["songs"],
    queryFn: () => stereodromeCore.getSongs(),
    enabled: stereodrome.offlineMode,
  });
  const shownArtists = visibleArtists(
    artists.data ?? [],
    songs.data ?? [],
    stereodrome.offlineMode,
    stereodrome.offlineSongIds
  );
  const isLoading =
    artists.isLoading || (stereodrome.offlineMode && songs.isLoading);

  async function playArtist(artistId: string) {
    const artistSongs = visibleSongs(
      await stereodromeCore.getSongs(undefined, artistId),
      stereodrome.offlineMode,
      stereodrome.offlineSongIds
    );
    if (artistSongs.length > 0) {
      await playback.playSong(artistSongs[0], artistSongs);
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
        onSelect: () =>
          view.push({
            name: "artist",
            title: artist.name,
            params: { artistId: artist.id, title: artist.name },
          }),
        onLongSelect: () => playArtist(artist.id),
      }))}
    />
  );
}
