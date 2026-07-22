import { useCallback, useEffect } from "react";
import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { coreClient } from "@/core/client";
import { queueItemFromSong } from "@/core/queue";
import {
  useFileState,
  usePlaybackActions,
  useStereodrome,
} from "@/core/selectors";
import { useSongActions } from "@/context/SongActionContext";
import { useViewStack } from "@/context/ViewContext";
import { songFileState, visibleSongs } from "@/services/offlineLibrary";

export function SongsScreen({ artistId }: { artistId?: string }) {
  const playback = usePlaybackActions();
  const { clearActiveSongTarget, setActiveSongTarget } = useSongActions();
  const stereodrome = useStereodrome();
  const fileState = useFileState();
  const view = useViewStack();
  const songs = useQuery({
    queryKey: artistId === undefined ? ["songs"] : ["artist-songs", artistId],
    queryFn: () =>
      coreClient.dispatchTyped({
        type: "get-songs",
        album_id: null,
        artist_id: artistId ?? null,
      }),
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
            : "No songs synced"
      }
      options={shownSongs.map((song) => ({
        label: song.title,
        fileState: songFileState(
          song.id,
          fileState.offlineSongIds,
          fileState.downloadingSongIds
        ),
        sublabel: [song.artist, song.album].filter(Boolean).join(" - "),
        onSelect: async () => {
          await playback.playSong(
            song,
            shownSongs.length ? shownSongs : [song]
          );
          view.showNowPlaying();
        },
        onLongSelect: async () => {
          await coreClient.dispatch({
            type: "insert-next",
            item: queueItemFromSong(song),
          });
        },
      }))}
      onActiveIndexChange={handleActiveIndexChange}
    />
  );
}
