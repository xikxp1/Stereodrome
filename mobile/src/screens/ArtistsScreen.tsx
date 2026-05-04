import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { stereodromeCore } from "@/services/stereodromeCore";
import { useViewStack } from "@/context/ViewContext";

export function ArtistsScreen() {
  const view = useViewStack();
  const playback = usePlayback();
  const artists = useQuery({
    queryKey: ["artists"],
    queryFn: stereodromeCore.getArtists,
  });

  async function playArtist(artistId: string) {
    const songs = await stereodromeCore.getSongs(undefined, artistId);
    if (songs.length > 0) {
      await playback.playSong(songs[0], songs);
      view.showNowPlaying();
    }
  }

  return (
    <SelectableList
      empty={artists.isLoading ? "Loading artists" : "No artists synced"}
      options={(artists.data ?? []).map((artist) => ({
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
