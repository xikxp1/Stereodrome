import { useQuery, useQueryClient } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { useSongActions } from "@/context/SongActionContext";
import { useStereodrome } from "@/core/selectors";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function SongPlaylistPickerScreen() {
  const queryClient = useQueryClient();
  const songActions = useSongActions();
  const stereodrome = useStereodrome();
  const view = useViewStack();
  const target = songActions.menuTarget;
  const playlists = useQuery({
    queryKey: ["playlists", stereodrome.offlineMode ? "offline" : "online"],
    queryFn: stereodromeCore.getPlaylists,
    enabled: !stereodrome.offlineMode,
  });

  if (!target) {
    return <SelectableList empty="No song selected" options={[]} />;
  }

  return (
    <SelectableList
      empty={playlists.isLoading ? "Loading playlists" : "No playlists"}
      options={(playlists.data ?? []).map((playlist) => {
        const isSourcePlaylist = playlist.id === target.sourcePlaylistId;
        return {
          label: playlist.name,
          sublabel: isSourcePlaylist
            ? "Already in this playlist"
            : `${playlist.song_count} songs`,
          disabled: isSourcePlaylist,
          onSelect: async () => {
            await stereodromeCore.addSongsToPlaylist(playlist.id, [
              target.song.id,
            ]);
            await queryClient.invalidateQueries({ queryKey: ["playlists"] });
            await queryClient.invalidateQueries({
              queryKey: ["playlist-songs", playlist.id],
            });
            view.pop();
            view.pop();
          },
        };
      })}
      resetSelectionKey={target.song.id}
    />
  );
}
