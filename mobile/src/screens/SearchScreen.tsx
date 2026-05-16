import { useEffect, useRef, useState } from "react";
import { Pressable, StyleSheet, Text, TextInput, View } from "react-native";
import { useQuery } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { colors } from "@/components/theme";
import { usePlayback } from "@/context/PlaybackContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function SearchScreen() {
  const inputRef = useRef<TextInput>(null);
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const playback = usePlayback();
  const view = useViewStack();
  const results = useQuery({
    queryKey: ["search", debouncedQuery],
    queryFn: () => stereodromeCore.searchLibrary(debouncedQuery, 20),
    enabled: debouncedQuery.trim().length > 1,
  });

  useEffect(() => {
    const focusTimeout = setTimeout(() => inputRef.current?.focus(), 120);
    return () => clearTimeout(focusTimeout);
  }, []);

  useEffect(() => {
    const timeout = setTimeout(() => {
      setDebouncedQuery(query.trim());
    }, 300);
    return () => clearTimeout(timeout);
  }, [query]);

  async function playAlbum(albumId: string) {
    const songs = await stereodromeCore.getSongs(albumId);
    if (songs.length > 0) {
      await playback.playSong(songs[0], songs);
      view.showNowPlaying();
    }
  }

  async function playArtist(artistId: string) {
    const songs = await stereodromeCore.getSongs(undefined, artistId);
    if (songs.length > 0) {
      await playback.playSong(songs[0], songs);
      view.showNowPlaying();
    }
  }

  const data = results.data;
  const options = [
    ...(data?.songs ?? []).map((song) => ({
      label: song.title,
      sublabel: song.artist ?? undefined,
      onSelect: async () => {
        await playback.playSong(song);
        view.showNowPlaying();
      },
    })),
    ...(data?.albums ?? []).map((album) => ({
      label: album.name,
      sublabel: album.artist ?? undefined,
      onSelect: () =>
        view.push({
          name: "album",
          title: album.name,
          params: { albumId: album.id, title: album.name },
        }),
      onLongSelect: () => playAlbum(album.id),
    })),
    ...(data?.artists ?? []).map((artist) => ({
      label: artist.name,
      sublabel: `${artist.album_count} albums`,
      onSelect: () =>
        view.push({
          name: "artist",
          title: artist.name,
          params: { artistId: artist.id, title: artist.name },
        }),
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
        <Pressable onPress={() => setQuery("")} style={styles.clear}>
          <Text style={styles.clearText}>x</Text>
        </Pressable>
      </View>
      <SelectableList
        empty={results.isLoading ? "Searching" : "Type to search"}
        options={options}
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
