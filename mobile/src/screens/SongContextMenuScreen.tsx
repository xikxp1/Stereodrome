import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  SelectableList,
  type SelectableOption,
} from "@/components/SelectableList";
import { usePlaybackMetadata, useStereodrome } from "@/core/selectors";
import { useSongActions } from "@/context/SongActionContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";
import type { Song } from "@/types/music";

function songSubtitle(song: { artist?: string | null; album?: string | null }) {
  return [song.artist, song.album].filter(Boolean).join(" - ");
}

export function SongContextMenuScreen() {
  const playback = usePlaybackMetadata();
  const songActions = useSongActions();
  const stereodrome = useStereodrome();
  const view = useViewStack();
  const target = songActions.menuTarget;
  const playlists = useQuery({
    queryKey: ["playlists", stereodrome.offlineMode ? "offline" : "online"],
    queryFn: stereodromeCore.getPlaylists,
    enabled: !stereodrome.offlineMode,
  });
  const songs = useQuery({
    queryKey: ["songs"],
    queryFn: () => stereodromeCore.getSongs(),
    enabled: target !== null && target.fullSong == null,
  });

  const fullSong = useMemo<Song | null>(() => {
    if (!target) {
      return null;
    }
    return (
      target.fullSong ??
      songs.data?.find((song) => song.id === target.song.id) ??
      null
    );
  }, [songs.data, target]);

  if (!target) {
    return <SelectableList empty="No song selected" options={[]} />;
  }

  const song = target.song;
  const isCurrentSong = playback.currentSong?.id === song.id;
  const isNextSong = playback.nextSong?.id === song.id;
  const isAlreadyQueued = playback.queue.some(
    (queuedSong) => queuedSong.id === song.id
  );
  const canAddToPlaylist =
    !stereodrome.offlineMode && (playlists.data?.length ?? 0) > 0;

  const options: SelectableOption[] = [
    {
      label: "Play Next",
      sublabel: songSubtitle(song),
      disabled: isCurrentSong || isNextSong,
      onSelect: async () => {
        await stereodromeCore.insertNext(song);
        view.pop();
      },
    },
    {
      label: "Add to Queue",
      sublabel: isAlreadyQueued ? "Already in queue" : songSubtitle(song),
      disabled: isAlreadyQueued,
      onSelect: async () => {
        await stereodromeCore.addToQueue(song);
        view.pop();
      },
    },
    {
      label: "Go to Artist",
      sublabel: fullSong?.artist ?? "Artist unavailable",
      disabled: fullSong?.artist_id == null || fullSong.artist_id.length === 0,
      onSelect: () => {
        const artistId = fullSong?.artist_id;
        if (artistId == null || artistId.length === 0) {
          return;
        }
        view.push({
          name: "artist",
          title: fullSong?.artist ?? "Artist",
          params: {
            artistId,
            title: fullSong?.artist ?? "Artist",
          },
        });
      },
    },
    {
      label: "Go to Album",
      sublabel: fullSong?.album ?? "Album unavailable",
      disabled: fullSong?.album_id == null || fullSong.album_id.length === 0,
      onSelect: () => {
        const albumId = fullSong?.album_id;
        if (albumId == null || albumId.length === 0) {
          return;
        }
        view.push({
          name: "album",
          title: fullSong?.album ?? "Album",
          params: {
            albumId,
            title: fullSong?.album ?? "Album",
          },
        });
      },
    },
    {
      label: "Add to Playlist",
      sublabel: stereodrome.offlineMode
        ? "Unavailable offline"
        : playlists.isLoading
          ? "Loading playlists"
          : canAddToPlaylist
            ? `${playlists.data?.length ?? 0} playlists`
            : "No playlists",
      disabled: !canAddToPlaylist,
      onSelect: () => {
        view.push({ name: "songPlaylistPicker", title: "Add to Playlist" });
      },
    },
  ];

  return <SelectableList options={options} resetSelectionKey={song.id} />;
}
