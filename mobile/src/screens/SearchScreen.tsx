import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Pressable, StyleSheet, Text, TextInput, View } from "react-native";
import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { colors } from "@/components/theme";
import { usePlaybackActions } from "@/context/PlaybackContext";
import { useSongActions } from "@/context/SongActionContext";
import { useFileState, useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import {
  songFileState,
  visibleSearchResults,
  visibleSongs,
} from "@/services/offlineLibrary";
import { stereodromeCore } from "@/services/stereodromeCore";

export function SearchScreen() {
  const inputRef = useRef<TextInput>(null);
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const playback = usePlaybackActions();
  const { clearActiveSongTarget, setActiveSongTarget } = useSongActions();
  const stereodrome = useStereodrome();
  const fileState = useFileState();
  const view = useViewStack();
  const results = useQuery({
    queryKey: ["search", debouncedQuery],
    queryFn: () => stereodromeCore.searchLibrary(debouncedQuery, 20),
    enabled: debouncedQuery.trim().length > 1,
  });
  const songs = useQuery({
    queryKey: ["songs"],
    queryFn: () => stereodromeCore.getSongs(),
    enabled: stereodrome.offlineMode,
  });

  useEffect(() => {
    const focusTimeout = setTimeout(() => inputRef.current?.focus(), 120);
    return () => {
      clearTimeout(focusTimeout);
    };
  }, []);

  useEffect(() => {
    const timeout = setTimeout(() => {
      setDebouncedQuery(query.trim());
    }, 300);
    return () => {
      clearTimeout(timeout);
    };
  }, [query]);

  async function playAlbum(albumId: string) {
    const albumSongs = visibleSongs(
      await stereodromeCore.getSongs(albumId),
      stereodrome.offlineMode,
      fileState.offlineSongIds
    );
    const firstSong = albumSongs[0];
    if (firstSong) {
      await playback.playSong(firstSong, albumSongs);
      view.showNowPlaying();
    }
  }

  async function playArtist(artistId: string) {
    const artistSongs = visibleSongs(
      await stereodromeCore.getSongs(undefined, artistId),
      stereodrome.offlineMode,
      fileState.offlineSongIds
    );
    const firstSong = artistSongs[0];
    if (firstSong) {
      await playback.playSong(firstSong, artistSongs);
      view.showNowPlaying();
    }
  }

  const data = visibleSearchResults(
    results.data,
    songs.data ?? [],
    stereodrome.offlineMode,
    fileState.offlineSongIds
  );
  const resultSongs = useMemo(() => data?.songs ?? [], [data?.songs]);
  const handleActiveIndexChange = useCallback(
    (index: number) => {
      const song = resultSongs[index] ?? null;
      setActiveSongTarget(
        song
          ? {
              song,
              origin: "list",
            }
          : null
      );
    },
    [resultSongs, setActiveSongTarget]
  );

  useEffect(
    () => () => {
      clearActiveSongTarget();
    },
    [clearActiveSongTarget]
  );

  const options = [
    ...resultSongs.map((song) => ({
      label: song.title,
      fileState: songFileState(
        song.id,
        fileState.offlineSongIds,
        fileState.downloadingSongIds
      ),
      ...(song.artist == null ? {} : { sublabel: song.artist }),
      onSelect: async () => {
        await playback.playSong(song);
        view.showNowPlaying();
      },
    })),
    ...(data?.albums ?? []).map((album) => ({
      label: album.name,
      ...(album.artist == null ? {} : { sublabel: album.artist }),
      onSelect: () => {
        view.push({
          name: "album",
          title: album.name,
          params: { albumId: album.id, title: album.name },
        });
      },
      onLongSelect: () => playAlbum(album.id),
    })),
    ...(data?.artists ?? []).map((artist) => ({
      label: artist.name,
      sublabel: `${artist.album_count} albums`,
      onSelect: () => {
        view.push({
          name: "artist",
          title: artist.name,
          params: { artistId: artist.id, title: artist.name },
        });
      },
      onLongSelect: () => playArtist(artist.id),
    })),
  ];

  return (
    <View style={styles.container}>
      <View style={styles.searchRow}>
        <TextInput
          ref={inputRef}
          autoCapitalize="none"
          autoFocus
          onChangeText={setQuery}
          placeholder="Search"
          style={styles.input}
          value={query}
        />
        <Pressable
          onPress={() => {
            setQuery("");
          }}
          style={styles.clear}
        >
          <Text style={styles.clearText}>x</Text>
        </Pressable>
      </View>
      <SelectableList
        empty={
          results.isLoading || (stereodrome.offlineMode && songs.isLoading)
            ? "Searching"
            : "Type to search"
        }
        options={options}
        onActiveIndexChange={handleActiveIndexChange}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
  },
  searchRow: {
    flexDirection: "row",
    gap: 5,
    padding: 6,
  },
  input: {
    backgroundColor: "#fff",
    borderColor: "#c9c9c1",
    borderRadius: 4,
    borderWidth: 1,
    flex: 1,
    height: 30,
    paddingHorizontal: 8,
  },
  clear: {
    alignItems: "center",
    backgroundColor: colors.selected,
    borderRadius: 4,
    height: 30,
    justifyContent: "center",
    width: 30,
  },
  clearText: {
    color: "#fff",
    fontWeight: "800",
  },
});
