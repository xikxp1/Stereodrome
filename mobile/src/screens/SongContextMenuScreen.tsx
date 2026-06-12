import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  SelectableList,
  type SelectableOption,
} from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useSongActions } from "@/context/SongActionContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";
import type { Song } from "@/types/music";

function songSubtitle(song: { artist?: string | null; album?: string | null }) {
  return [song.artist, song.album].filter(Boolean).join(" - ");
}

export function SongContextMenuScreen() {
  const playback = usePlayback();
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
    enabled: !!target && !target.fullSong,
  });

  const fullSong = useMemo<Song | null>(() => {
    if (!target) return null;
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
      disabled: !fullSong?.artist_id,
      onSelect: () => {
        if (!fullSong?.artist_id) return;
        view.push({
          name: "artist",
          title: fullSong.artist ?? "Artist",
          params: {
            artistId: fullSong.artist_id,
            title: fullSong.artist ?? "Artist",
          },
        });
      },
    },
    {
      label: "Go to Album",
      sublabel: fullSong?.album ?? "Album unavailable",
      disabled: !fullSong?.album_id,
      onSelect: () => {
        if (!fullSong?.album_id) return;
        view.push({
          name: "album",
          title: fullSong.album ?? "Album",
          params: {
            albumId: fullSong.album_id,
            title: fullSong.album ?? "Album",
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
