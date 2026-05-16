import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { visibleSongs } from "@/services/offlineLibrary";
import { stereodromeCore } from "@/services/stereodromeCore";

export function SongsScreen() {
  const playback = usePlayback();
  const stereodrome = useStereodrome();
  const view = useViewStack();
  const songs = useQuery({
    queryKey: ["songs"],
    queryFn: () => stereodromeCore.getSongs(),
  });
  const shownSongs = visibleSongs(
    songs.data ?? [],
    stereodrome.offlineMode,
    stereodrome.offlineSongIds
  );

  return (
    <SelectableList
      empty={
        songs.isLoading
          ? "Loading songs"
          : stereodrome.offlineMode
            ? "No offline songs"
            : "No songs synced"
      }
      options={shownSongs.map((song) => ({
        label: song.title,
        sublabel: [song.artist, song.album].filter(Boolean).join(" - "),
        onSelect: async () => {
          await playback.playSong(
            song,
            shownSongs.length ? shownSongs : [song]
          );
          view.showNowPlaying();
        },
        onLongSelect: async () => {
          await stereodromeCore.insertNext(song);
        },
      }))}
    />
  );
}
