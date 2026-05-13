import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function SongsScreen() {
  const playback = usePlayback();
  const view = useViewStack();
  const songs = useQuery({
    queryKey: ["songs"],
    queryFn: () => stereodromeCore.getSongs(),
  });

  return (
    <SelectableList
      empty={songs.isLoading ? "Loading songs" : "No songs synced"}
      options={(songs.data ?? []).map((song) => ({
        label: song.title,
        sublabel: [song.artist, song.album].filter(Boolean).join(" - "),
        onSelect: async () => {
          await playback.playSong(song, songs.data ?? [song]);
          view.showNowPlaying();
        },
      }))}
    />
  );
}
