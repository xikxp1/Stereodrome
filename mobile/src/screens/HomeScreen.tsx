import { SelectableList } from "@/components/SelectableList";
import { usePlaybackMetadata } from "@/context/PlaybackContext";
import { useViewStack } from "@/context/ViewContext";

export function HomeScreen() {
  const view = useViewStack();
  const playback = usePlaybackMetadata();

  return (
    <SelectableList
      options={[
        ...(playback.currentSong
          ? [
              {
                label: "Now Playing",
                onSelect: () => {
                  view.showNowPlaying();
                },
              },
            ]
          : []),
        {
          label: "Music",
          onSelect: () => {
            view.push({ name: "music", title: "Music" });
          },
        },
        {
          label: "Search",
          onSelect: () => {
            view.push({ name: "search", title: "Search" });
          },
        },
        {
          label: "Downloads",
          onSelect: () => {
            view.push({ name: "downloads", title: "Downloads" });
          },
        },
        {
          label: "Queue",
          onSelect: () => {
            view.push({ name: "queue", title: "Queue" });
          },
        },
        {
          label: "Settings",
          onSelect: () => {
            view.push({ name: "settings", title: "Settings" });
          },
        },
      ]}
    />
  );
}
