import { SelectableList } from "@/components/SelectableList";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";

export function MusicScreen() {
  const stereodrome = useStereodrome();
  const view = useViewStack();

  return (
    <SelectableList
      options={[
        {
          label: "Artists",
          onSelect: () => view.push({ name: "artists", title: "Artists" }),
        },
        {
          label: "Albums",
          onSelect: () => view.push({ name: "albums", title: "Albums" }),
        },
        {
          label: "Songs",
          onSelect: () => view.push({ name: "songs", title: "Songs" }),
        },
        ...(stereodrome.offlineMode
          ? []
          : [
              {
                label: "Recently Added",
                onSelect: () =>
                  view.push({
                    name: "albumList",
                    title: "Recently Added",
                    params: { kind: "recentlyAdded" },
                  }),
              },
              {
                label: "Recently Played",
                onSelect: () =>
                  view.push({
                    name: "albumList",
                    title: "Recently Played",
                    params: { kind: "recentlyPlayed" },
                  }),
              },
              {
                label: "Most Played",
                onSelect: () =>
                  view.push({
                    name: "albumList",
                    title: "Most Played",
                    params: { kind: "mostPlayed" },
                  }),
              },
              {
                label: "Playlists",
                onSelect: () =>
                  view.push({ name: "playlists", title: "Playlists" }),
              },
            ]),
      ]}
    />
  );
}
