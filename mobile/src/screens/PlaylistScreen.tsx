import { useCallback, useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import {
  SelectableList,
  type SelectableOption,
} from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useSongActions } from "@/context/SongActionContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { visibleSongs } from "@/services/offlineLibrary";
import { stereodromeCore } from "@/services/stereodromeCore";

export function PlaylistScreen({
  playlistId,
}: {
  playlistId: string;
  title: string;
}) {
  const playback = usePlayback();
  const { clearActiveSongTarget, setActiveSongTarget } = useSongActions();
  const stereodrome = useStereodrome();
  const view = useViewStack();
  const queryClient = useQueryClient();
  const [confirmingRemoval, setConfirmingRemoval] = useState(false);
  const songs = useQuery({
    queryKey: ["playlist-songs", playlistId],
    queryFn: () => stereodromeCore.getPlaylistSongs(playlistId),
    enabled: !!playlistId,
  });
  const playlists = useQuery({
    queryKey: ["playlists", stereodrome.offlineMode ? "offline" : "online"],
    queryFn: stereodromeCore.getPlaylists,
  });
  const playlist = (playlists.data ?? []).find(
    (item) => item.id === playlistId
  );
  const savedOffline = playlist?.saved_offline ?? false;
  const shownSongs = visibleSongs(
    songs.data ?? [],
    stereodrome.offlineMode,
    stereodrome.offlineSongIds
  );
  const songOptionOffset = stereodrome.offlineMode
    ? 0
    : confirmingRemoval && savedOffline
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

  useEffect(() => {
    return () => clearActiveSongTarget();
  }, [clearActiveSongTarget]);

  useEffect(() => {
    setConfirmingRemoval(false);
  }, [playlistId, savedOffline, stereodrome.offlineMode]);

  async function setSavedOffline(nextSavedOffline: boolean) {
    if (!playlistId || stereodrome.offlineMode) {
      return;
    }
    setConfirmingRemoval(false);
    await stereodromeCore.setPlaylistSavedOffline(playlistId, nextSavedOffline);
    await queryClient.invalidateQueries({ queryKey: ["playlists"] });
    if (nextSavedOffline) {
      await stereodrome.reconcileSavedPlaylistsOffline();
    } else {
      await stereodrome.refreshOfflineSongIds();
    }
  }

  const actionOptions: SelectableOption[] =
    stereodrome.offlineMode
      ? []
      : confirmingRemoval && savedOffline
        ? [
            {
              label: "Cancel Removal",
              sublabel: "Keep playlist saved offline",
              onSelect: () => setConfirmingRemoval(false),
            },
            {
              label: "Confirm Remove",
              sublabel: "Use wheel select to remove offline save",
              wheelOnly: true,
              onSelect: () => setSavedOffline(false),
            },
          ]
        : [
            {
              label: savedOffline ? "Remove Offline Save" : "Save Offline",
              sublabel: savedOffline
                ? "Requires wheel confirmation"
                : "Download and preserve playlist songs",
              onSelect: () => {
                if (savedOffline) {
                  setConfirmingRemoval(true);
                  return;
                }
                void setSavedOffline(true);
              },
            },
          ];

  const options = [
    ...actionOptions,
    ...shownSongs.map((song) => ({
      label: song.title,
      sublabel: song.artist ?? undefined,
      onSelect: async () => {
        await playback.playSong(song, shownSongs.length ? shownSongs : [song]);
        view.showNowPlaying();
      },
      onLongSelect: stereodrome.offlineMode
        ? undefined
        : () => {
            if (savedOffline) {
              setConfirmingRemoval(true);
              return;
            }
            void setSavedOffline(true);
          },
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
