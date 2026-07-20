import { useEffect, useState } from "react";
import { Image, StyleSheet, Text, View } from "react-native";

import { SyncedMarqueeText } from "@/components/SyncedMarqueeText";
import { colors } from "@/components/theme";
import { usePlayback } from "@/context/PlaybackContext";
import { stereodromeCore } from "@/services/stereodromeCore";

type CoverArtState = {
  songId: string;
  uri: string | null;
};

function useCoverArtUri(songId: string | null, size: number) {
  const [coverArt, setCoverArt] = useState<CoverArtState | null>(null);

  useEffect(() => {
    if (songId === null || songId.length === 0) {
      return undefined;
    }

    let cancelled = false;
    stereodromeCore
      .getSongCoverArtUri(songId, size)
      .then((uri) => {
        if (!cancelled) {
          setCoverArt({ songId, uri });
        }
      })
      .catch(() => {
        if (!cancelled) {
          setCoverArt({ songId, uri: null });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [size, songId]);

  return coverArt?.songId === songId ? coverArt.uri : null;
}

export function NowPlayingScreen() {
  const playback = usePlayback();
  const song = playback.currentSong;
  const nextSong = playback.nextSong;
  const songId = song?.id ?? null;
  const nextSongId = nextSong?.id ?? null;
  const coverUri = useCoverArtUri(songId, 512);
  const nextCoverUri = useCoverArtUri(nextSongId, 128);
  const duration =
    playback.duration > 0 ? playback.duration : (song?.duration ?? 0);
  const progressRatio =
    song !== null && duration > 0
      ? Math.min(1, playback.position / duration)
      : 0;

  return (
    <View style={styles.container}>
      <View style={styles.currentRegion}>
        {coverUri !== null && coverUri.length > 0 ? (
          <Image source={{ uri: coverUri }} style={styles.cover} />
        ) : (
          <View style={styles.coverPlaceholder} />
        )}
        <SyncedMarqueeText
          group="mobile-now-playing"
          text={song?.title ?? "Nothing Playing"}
          style={styles.title}
        />
        <SyncedMarqueeText
          group="mobile-now-playing"
          text={[song?.artist, song?.album].filter(Boolean).join(" - ")}
          style={styles.subtitle}
        />
        <View
          accessibilityLabel="Playback position"
          accessibilityRole="progressbar"
          accessibilityValue={{
            min: 0,
            max: Math.max(0, duration),
            now: Math.max(0, Math.min(playback.position, duration)),
          }}
          pointerEvents="none"
          style={styles.progressTrack}
        >
          <View
            style={[styles.progressFill, { width: `${progressRatio * 100}%` }]}
          />
        </View>
      </View>
      <View style={styles.nextSlot}>
        {nextSong ? (
          <View style={styles.nextRow}>
            {nextCoverUri !== null && nextCoverUri.length > 0 ? (
              <Image source={{ uri: nextCoverUri }} style={styles.nextCover} />
            ) : (
              <View style={styles.nextCoverPlaceholder} />
            )}
            <View style={styles.nextText}>
              <SyncedMarqueeText
                align="left"
                containerStyle={styles.nextMarquee}
                group="mobile-next-song"
                text={nextSong.title}
                style={styles.nextTitle}
              />
              <SyncedMarqueeText
                align="left"
                containerStyle={styles.nextMarquee}
                group="mobile-next-song"
                text={[nextSong.artist, nextSong.album]
                  .filter(Boolean)
                  .join(" - ")}
                style={styles.nextSubtitle}
              />
            </View>
          </View>
        ) : null}
      </View>
      {playback.error !== null && playback.error.length > 0 ? (
        <Text numberOfLines={2} style={styles.error}>
          {playback.error}
        </Text>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    alignItems: "center",
    flex: 1,
    padding: 12,
  },
  currentRegion: {
    alignItems: "center",
    flex: 1,
    justifyContent: "center",
    width: "100%",
  },
  cover: {
    aspectRatio: 1,
    borderRadius: 4,
    marginBottom: 12,
    width: "80%",
  },
  coverPlaceholder: {
    aspectRatio: 1,
    backgroundColor: "#d8d8d0",
    borderRadius: 4,
    marginBottom: 12,
    width: "80%",
  },
  title: {
    color: colors.text,
    fontSize: 16,
    fontWeight: "800",
    marginTop: 6,
  },
  subtitle: {
    color: colors.muted,
    fontSize: 12,
    fontWeight: "700",
  },
  progressFill: {
    backgroundColor: colors.selected,
    borderRadius: 2,
    height: "100%",
  },
  progressTrack: {
    backgroundColor: "#d8d8d0",
    borderColor: "#b9b9b2",
    borderRadius: 3,
    borderWidth: 1,
    height: 7,
    marginTop: 8,
    overflow: "hidden",
    width: "72%",
  },
  nextCover: {
    borderRadius: 3,
    height: 42,
    width: 42,
  },
  nextCoverPlaceholder: {
    backgroundColor: "#d8d8d0",
    borderRadius: 3,
    height: 42,
    width: 42,
  },
  nextMarquee: {
    height: 18,
  },
  nextRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: 8,
    width: "100%",
  },
  nextSlot: {
    flexShrink: 0,
    height: 42,
    justifyContent: "flex-end",
    width: "100%",
  },
  nextSubtitle: {
    color: colors.muted,
    fontSize: 11,
    fontWeight: "700",
  },
  nextText: {
    flex: 1,
    minWidth: 0,
  },
  nextTitle: {
    color: colors.text,
    fontSize: 12,
    fontWeight: "800",
  },
  error: {
    color: "#b3261e",
    fontSize: 11,
    fontWeight: "700",
    marginTop: 8,
    textAlign: "center",
  },
});
