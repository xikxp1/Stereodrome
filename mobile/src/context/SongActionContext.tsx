import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
} from "react";

import { usePlaybackMetadata } from "@/core/selectors";
import { useViewStack, type ViewName } from "@/context/ViewContext";
import type { PlayableSong, Song } from "@/types/music";

export type SongActionTarget = {
  song: PlayableSong;
  fullSong?: Song | null;
  sourcePlaylistId?: string | null;
  origin: "list" | "nowPlaying";
};

type SongActionContextValue = {
  activeListTarget: SongActionTarget | null;
  menuTarget: SongActionTarget | null;
  canOpenSongContextMenu: boolean;
  setActiveSongTarget(target: SongActionTarget | null): void;
  clearActiveSongTarget(): void;
  openSongContextMenu(): void;
};

const SongActionContext = createContext<SongActionContextValue | null>(null);

const songListViews = new Set<ViewName>([
  "album",
  "playlist",
  "search",
  "songs",
]);

function isSameTarget(
  left: SongActionTarget | null,
  right: SongActionTarget | null
) {
  return (
    left?.song.id === right?.song.id &&
    left?.sourcePlaylistId === right?.sourcePlaylistId &&
    left?.origin === right?.origin
  );
}

export function SongActionProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const playback = usePlaybackMetadata();
  const view = useViewStack();
  const { current, push } = view;
  const [activeListTarget, setActiveListTarget] =
    useState<SongActionTarget | null>(null);
  const [menuTarget, setMenuTarget] = useState<SongActionTarget | null>(null);

  const resolvedTarget = useMemo(() => {
    if (current.name === "nowPlaying" && playback.currentSong) {
      return {
        song: playback.currentSong,
        origin: "nowPlaying" as const,
      };
    }

    if (songListViews.has(current.name)) {
      return activeListTarget;
    }

    return null;
  }, [activeListTarget, current.name, playback.currentSong]);

  const setActiveSongTarget = useCallback((target: SongActionTarget | null) => {
    setActiveListTarget((currentTarget) =>
      isSameTarget(currentTarget, target) ? currentTarget : target
    );
  }, []);

  const clearActiveSongTarget = useCallback(() => {
    setActiveListTarget(null);
  }, []);

  const openSongContextMenu = useCallback(() => {
    if (!resolvedTarget) {
      return;
    }

    setMenuTarget(resolvedTarget);
    push({ name: "songContextMenu", title: "Song Actions" });
  }, [push, resolvedTarget]);

  const value = useMemo(
    () => ({
      activeListTarget,
      menuTarget,
      canOpenSongContextMenu: resolvedTarget !== null,
      setActiveSongTarget,
      clearActiveSongTarget,
      openSongContextMenu,
    }),
    [
      activeListTarget,
      clearActiveSongTarget,
      menuTarget,
      openSongContextMenu,
      resolvedTarget,
      setActiveSongTarget,
    ]
  );

  return (
    <SongActionContext.Provider value={value}>
      {children}
    </SongActionContext.Provider>
  );
}

export function useSongActions() {
  const value = useContext(SongActionContext);
  if (!value) {
    throw new Error("useSongActions must be used inside SongActionProvider");
  }
  return value;
}
