import { useInfiniteQuery } from "@tanstack/react-query";
import { useCallback, useMemo } from "react";

import { SelectableList } from "@/components/SelectableList";
import { usePlayback } from "@/context/PlaybackContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";
import type { AlbumListEntry } from "@/types/music";

export type RankedAlbumListKind =
  "recentlyAdded" | "recentlyPlayed" | "mostPlayed";

type RankedAlbumListConfig = {
  empty: string;
  listType: "newest" | "recent" | "frequent";
  loading: string;
};

const pageSize = 50;

const albumListConfig: Record<RankedAlbumListKind, RankedAlbumListConfig> = {
  recentlyAdded: {
    empty: "No recently added albums",
    listType: "newest",
    loading: "Loading recent albums",
  },
  recentlyPlayed: {
    empty: "No recently played albums",
    listType: "recent",
    loading: "Loading played albums",
  },
  mostPlayed: {
    empty: "No most played albums",
    listType: "frequent",
    loading: "Loading most played albums",
  },
};

export function isRankedAlbumListKind(
  value: string | undefined
): value is RankedAlbumListKind {
  return Boolean(value && value in albumListConfig);
}

export function AlbumListScreen({ kind }: { kind: RankedAlbumListKind }) {
  const playback = usePlayback();
  const view = useViewStack();
  const config = albumListConfig[kind];
  const albums = useInfiniteQuery({
    queryKey: ["album-list", kind],
    queryFn: ({ pageParam }) =>
      stereodromeCore.getAlbumList(config.listType, pageSize, pageParam),
    initialPageParam: 0,
    getNextPageParam: (lastPage, pages) =>
      lastPage.length < pageSize ? undefined : pages.length * pageSize,
  });
  const shownAlbums = useMemo(
    () => albums.data?.pages.flat() ?? [],
    [albums.data]
  );

  const playAlbum = useCallback(
    async (albumId: string) => {
      const songs = await stereodromeCore.getSongs(albumId);
      const firstSong = songs[0];
      if (firstSong) {
        await playback.playSong(firstSong, songs);
        view.showNowPlaying();
      }
    },
    [playback, view]
  );

  const options = useMemo(
    () =>
      shownAlbums.map((album: AlbumListEntry) => ({
        label: album.name,
        sublabel: albumSublabel(album),
        onSelect: () =>
          view.push({
            name: "album",
            title: album.name,
            params: { albumId: album.id, title: album.name },
          }),
        onLongSelect: () => playAlbum(album.id),
      })),
    [playAlbum, shownAlbums, view]
  );

  const { fetchNextPage, hasNextPage, isFetchingNextPage } = albums;
  const fetchMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage) {
      void fetchNextPage();
    }
  }, [fetchNextPage, hasNextPage, isFetchingNextPage]);

  return (
    <SelectableList
      empty={
        albums.isLoading
          ? config.loading
          : albums.isError
            ? "Failed to load albums"
            : config.empty
      }
      loadingMore={albums.isFetchingNextPage}
      onEndReached={fetchMore}
      options={options}
      preserveSelectionOnChange
      resetSelectionKey={kind}
    />
  );
}

function albumSublabel(album: AlbumListEntry) {
  const detail = [album.artist_name, album.year ? String(album.year) : null]
    .filter(Boolean)
    .join(" - ");
  if (album.play_count != null) {
    return detail
      ? `${detail} - ${album.play_count.toLocaleString()} plays`
      : `${album.play_count.toLocaleString()} plays`;
  }
  return detail || undefined;
}
