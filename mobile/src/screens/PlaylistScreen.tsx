import { useCallback, useEffect } from "react";
import { Alert } from "react-native";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import {
  SelectableList,
  type SelectableOption,
} from "@/components/SelectableList";
import { useProtectedSelectableAction } from "@/components/protectedSelectableAction";
import { coreClient } from "@/core/client";
import {
  useFileState,
  usePlaybackActions,
  useStereodrome,
} from "@/core/selectors";
import { useSongActions } from "@/context/SongActionContext";
import { useViewStack } from "@/context/ViewContext";
import { songFileState, visibleSongs } from "@/services/offlineLibrary";

export function PlaylistScreen({
  playlistId,
}: {
  playlistId: string;
  title: string;
}) {
  const playback = usePlaybackActions();
  const { clearActiveSongTarget, setActiveSongTarget } = useSongActions();
  const stereodrome = useStereodrome();
  const fileState = useFileState();
  const view = useViewStack();
  const queryClient = useQueryClient();
  const songs = useQuery({
    queryKey: ["playlist-songs", playlistId],
    queryFn: () =>
      coreClient.dispatchTyped({
        type: "get-playlist-songs",
        playlist_id: playlistId,
      }),
    enabled: Boolean(playlistId),
  });
  const playlists = useQuery({
    queryKey: ["playlists", stereodrome.offlineMode ? "offline" : "online"],
    queryFn: () => coreClient.dispatchTyped({ type: "get-playlists" }),
  });
  const playlist = (playlists.data ?? []).find(
    (item) => item.id === playlistId
  );
  const savedOffline = playlist?.saved_offline ?? false;
  const { armProtectedAction, pendingActionId, protectedActionRows } =
    useProtectedSelectableAction(
      `${playlistId}:${savedOffline}:${stereodrome.offlineMode}`
    );
  const shownSongs = visibleSongs(
    songs.data ?? [],
    stereodrome.offlineMode,
    fileState.offlineSongIds
  );
  const songOptionOffset = stereodrome.offlineMode
    ? 0
    : pendingActionId === "remove-offline-save" && savedOffline
      ? 2
      : 1;
  const handleActiveIndexChange = useCallback(
    (index: number) => {
      const song = shownSongs[index - songOptionOffset] ?? null;
      setActiveSongTarget(
        song
          ? {
              song,
              fullSong: song,
              sourcePlaylistId: playlistId,
              origin: "list",
            }
          : null
      );
    },
    [playlistId, shownSongs, setActiveSongTarget, songOptionOffset]
  );

  useEffect(
    () => () => {
      clearActiveSongTarget();
    },
    [clearActiveSongTarget]
  );

  async function setSavedOffline(nextSavedOffline: boolean) {
    if (!playlistId || stereodrome.offlineMode) {
      return;
    }
    try {
      await coreClient.dispatchTyped({
        type: "set-playlist-saved-offline",
        playlist_id: playlistId,
        saved_offline: nextSavedOffline,
      });
      await queryClient.invalidateQueries({ queryKey: ["playlists"] });
    } catch (saveError) {
      Alert.alert(
        "Offline save failed",
        saveError instanceof Error ? saveError.message : String(saveError)
      );
    }
  }

  const actionOptions: SelectableOption[] = stereodrome.offlineMode
    ? []
    : savedOffline
      ? protectedActionRows({
          id: "remove-offline-save",
          label: "Remove Offline Save",
          sublabel: "Requires wheel confirmation",
          confirmLabel: "Confirm Remove",
          confirmSublabel: "Use wheel select to remove offline save",
          cancelLabel: "Cancel Removal",
          cancelSublabel: "Keep playlist saved offline",
          onConfirm: () => setSavedOffline(false),
        })
      : [
          {
            label: "Save Offline",
            sublabel: "Download and preserve playlist songs",
            onSelect: () => setSavedOffline(true),
          },
        ];

  const options = [
    ...actionOptions,
    ...shownSongs.map((song) => ({
      label: song.title,
      fileState: songFileState(
        song.id,
        fileState.offlineSongIds,
        fileState.downloadingSongIds
      ),
      ...(song.artist == null ? {} : { sublabel: song.artist }),
      onSelect: async () => {
        await playback.playSong(song, shownSongs.length ? shownSongs : [song]);
        view.showNowPlaying();
      },
      ...(stereodrome.offlineMode
        ? {}
        : {
            onLongSelect: () => {
              if (savedOffline) {
                armProtectedAction("remove-offline-save");
                return;
              }
              void setSavedOffline(true);
            },
          }),
    })),
  ];

  return (
    <SelectableList
      empty={
        songs.isLoading
          ? "Loading playlist"
          : stereodrome.offlineMode
            ? "No offline playlist songs"
            : "No songs"
      }
      options={options}
      onActiveIndexChange={handleActiveIndexChange}
    />
  );
}
