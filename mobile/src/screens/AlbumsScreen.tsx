import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function AlbumsScreen() {
  const view = useViewStack();
  const playback = usePlayback();
  const albums = useQuery({
    queryKey: ["albums"],
    queryFn: () => stereodromeCore.getAlbums(),
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
      empty={albums.isLoading ? "Loading albums" : "No albums synced"}
      options={(albums.data ?? []).map((album) => ({
        label: album.name,
        sublabel: album.artist_name ?? undefined,
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
