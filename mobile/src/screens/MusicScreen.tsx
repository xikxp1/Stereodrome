import { SelectableList } from "@/components/SelectableList";
import { useViewStack } from "@/context/ViewContext";

export function MusicScreen() {
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
        {
          label: "Playlists",
          onSelect: () => view.push({ name: "playlists", title: "Playlists" }),
        },
      ]}
    />
  );
}
