import { useCallback, useEffect } from "react";
import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { coreClient } from "@/core/client";
import {
  useFileState,
  usePlaybackActions,
  useStereodrome,
} from "@/core/selectors";
import { useSongActions } from "@/context/SongActionContext";
import { useViewStack } from "@/context/ViewContext";
import { songFileState, visibleSongs } from "@/services/offlineLibrary";

export function AlbumScreen({ albumId }: { albumId: string; title: string }) {
  const playback = usePlaybackActions();
  const { clearActiveSongTarget, setActiveSongTarget } = useSongActions();
  const stereodrome = useStereodrome();
  const fileState = useFileState();
  const view = useViewStack();
  const songs = useQuery({
    queryKey: ["album-songs", albumId],
    queryFn: () =>
      coreClient.dispatchTyped({
        type: "get-songs",
        album_id: albumId,
        artist_id: null,
      }),
    enabled: Boolean(albumId),
  });
  const shownSongs = visibleSongs(
    songs.data ?? [],
    stereodrome.offlineMode,
    fileState.offlineSongIds
  );
  const handleActiveIndexChange = useCallback(
    (index: number) => {
      const song = shownSongs[index] ?? null;
      setActiveSongTarget(
        song
          ? {
              song,
              fullSong: song,
              origin: "list",
            }
          : null
      );
    },
    [shownSongs, setActiveSongTarget]
  );

  useEffect(
    () => () => {
      clearActiveSongTarget();
    },
    [clearActiveSongTarget]
  );

  return (
    <SelectableList
      empty={
        songs.isLoading
          ? "Loading songs"
          : stereodrome.offlineMode
            ? "No offline songs"
            : "No songs"
      }
      options={shownSongs.map((song) => ({
        label: song.title,
        fileState: songFileState(
          song.id,
          fileState.offlineSongIds,
          fileState.downloadingSongIds
        ),
        ...(song.artist == null ? {} : { sublabel: song.artist }),
        onSelect: async () => {
          await playback.playSong(
            song,
            shownSongs.length ? shownSongs : [song]
          );
          view.showNowPlaying();
        },
        ...(stereodrome.offlineMode
          ? {}
          : {
              onLongSelect: async () => {
                await coreClient.dispatchTyped({
                  type: "download-album",
                  album_id: albumId,
                });
              },
            }),
      }))}
      onActiveIndexChange={handleActiveIndexChange}
    />
  );
}
