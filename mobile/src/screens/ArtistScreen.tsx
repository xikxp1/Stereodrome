import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function ArtistScreen({
  artistId,
}: {
  artistId: string;
  title: string;
}) {
  const view = useViewStack();
  const playback = usePlayback();
  const albums = useQuery({
    queryKey: ["artist-albums", artistId],
    queryFn: () => stereodromeCore.getAlbums(artistId),
    enabled: !!artistId,
  });

  async function playAlbum(albumId: string) {
    const songs = await stereodromeCore.getSongs(albumId);
    if (songs.length > 0) {
      await playback.playSong(songs[0], songs);
      view.showNowPlaying();
    }
  }

  return (
    <SelectableList
      empty={albums.isLoading ? "Loading albums" : "No albums"}
      options={(albums.data ?? []).map((album) => ({
        label: album.name,
        sublabel: album.year ? String(album.year) : undefined,
        onSelect: () =>
          view.push({
            name: "album",
            title: album.name,
            params: { albumId: album.id, title: album.name },
          }),
        onLongSelect: () => playAlbum(album.id),
      }))}
    />
  );
}
