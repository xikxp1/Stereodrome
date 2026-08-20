import { useEffect, useState } from "react";
import { Image, StyleSheet, View } from "react-native";

import { SyncedMarqueeText } from "@/components/SyncedMarqueeText";
import { SongFileStateIcon } from "@/components/SongFileStateIcon";
import { colors } from "@/components/theme";
import { coreClient } from "@/core/client";
import {
  useFileState,
  usePlayback,
  usePlaybackPosition,
} from "@/core/selectors";
import { songFileState } from "@/services/offlineLibrary";

type CoverArtState = {
  songId: string;
  uri: string | null;
};

const wideLayoutAspectRatio = 1.35;
const wideCoverWidthRatio = 0.42;
const wideCoverVerticalInset = 8;
const stackedDetailsHeight = 66;

function useCoverArtUri(songId: string | null, size: number) {
  const [coverArt, setCoverArt] = useState<CoverArtState | null>(null);

  useEffect(() => {
    if (songId === null || songId.length === 0) {
      return undefined;
    }

    let cancelled = false;
    coreClient
      .dispatchTyped({
        type: "get-song-cover-art-uri",
        id: songId,
        size,
      })
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
  const position = usePlaybackPosition();
  const fileState = useFileState();
  const [currentRegionSize, setCurrentRegionSize] = useState({
    height: 0,
    width: 0,
  });
  const song = playback.currentSong;
  const nextSong = playback.upcomingSong;
  const songId = song?.id ?? null;
  const nextSongId = nextSong?.id ?? null;
  const nextSongFileState =
    nextSong === null
      ? null
      : songFileState(
          nextSong.id,
          fileState.offlineSongIds,
          fileState.downloadingSongIds
        );
  const coverUri = useCoverArtUri(songId, 512);
  const nextCoverUri = useCoverArtUri(nextSongId, 128);
  const duration =
    playback.duration > 0 ? playback.duration : (song?.duration ?? 0);
  const progressRatio =
    song !== null && duration > 0 ? Math.min(1, position / duration) : 0;
  const usesWideLayout =
    currentRegionSize.width > 0 &&
    currentRegionSize.width >= currentRegionSize.height * wideLayoutAspectRatio;
  const coverSize =
    currentRegionSize.width > 0
      ? Math.max(
          0,
          Math.min(
            currentRegionSize.width *
              (usesWideLayout ? wideCoverWidthRatio : 0.8),
            usesWideLayout
              ? currentRegionSize.height - wideCoverVerticalInset
              : currentRegionSize.height - stackedDetailsHeight
          )
        )
      : null;
  const measuredCoverStyle =
    coverSize === null ? null : { height: coverSize, width: coverSize };

  return (
    <View style={styles.container}>
      <View
        onLayout={(event) => {
          const { height, width } = event.nativeEvent.layout;
          setCurrentRegionSize((current) =>
            current.height === height && current.width === width
              ? current
              : { height, width }
          );
        }}
        style={[
          styles.currentRegion,
          usesWideLayout && styles.currentRegionWide,
        ]}
      >
        {coverUri !== null && coverUri.length > 0 ? (
          <Image
            source={{ uri: coverUri }}
            style={[
              styles.cover,
              measuredCoverStyle,
              usesWideLayout && styles.coverWide,
            ]}
          />
        ) : (
          <View
            style={[
              styles.coverPlaceholder,
              measuredCoverStyle,
              usesWideLayout && styles.coverWide,
            ]}
          />
        )}
        <View
          style={[
            styles.currentDetails,
            usesWideLayout && styles.currentDetailsWide,
          ]}
        >
          <SyncedMarqueeText
            align={usesWideLayout ? "left" : "center"}
            group="mobile-now-playing"
            text={song?.title ?? "Nothing Playing"}
            style={styles.title}
          />
          <SyncedMarqueeText
            align={usesWideLayout ? "left" : "center"}
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
              now: Math.max(0, Math.min(position, duration)),
            }}
            pointerEvents="none"
            style={[
              styles.progressTrack,
              usesWideLayout && styles.progressTrackWide,
            ]}
          >
            <View
              style={[
                styles.progressFill,
                { width: `${progressRatio * 100}%` },
              ]}
            />
          </View>
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
              <View style={styles.nextTitleRow}>
                {nextSongFileState === null ? null : (
                  <SongFileStateIcon state={nextSongFileState} />
                )}
                <SyncedMarqueeText
                  align="left"
                  containerStyle={styles.nextTitleMarquee}
                  group="mobile-next-song"
                  text={nextSong.title}
                  style={styles.nextTitle}
                />
              </View>
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
  currentRegionWide: {
    flexDirection: "row",
    gap: 12,
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
  coverWide: {
    marginBottom: 0,
  },
  currentDetails: {
    alignItems: "center",
    width: "100%",
  },
  currentDetailsWide: {
    flex: 1,
    minWidth: 0,
    width: 0,
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
  progressTrackWide: {
    width: "100%",
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
  nextTitleMarquee: {
    flex: 1,
    height: 18,
    minWidth: 0,
  },
  nextTitleRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: 5,
  },
});
