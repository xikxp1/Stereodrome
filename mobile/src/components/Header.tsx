import { StyleSheet, Text, View } from "react-native";

import { SyncedMarqueeText } from "@/components/SyncedMarqueeText";
import { colors } from "@/components/theme";
import { usePlayback, usePlaybackPosition } from "@/core/selectors";

function formatPlaybackTime(seconds: number) {
  const safeSeconds = Math.max(0, Math.floor(seconds));
  const minutes = Math.floor(safeSeconds / 60);
  const remainingSeconds = safeSeconds % 60;
  return `${minutes}:${remainingSeconds.toString().padStart(2, "0")}`;
}

// Leaf consumer of the 1 Hz interpolated position: only this small text node
// re-renders every second instead of the whole shell tree.
function HeaderPlaybackTime() {
  const playback = usePlayback();
  const position = usePlaybackPosition();
  if (!playback.currentSong) {
    return null;
  }
  const duration =
    playback.duration > 0
      ? playback.duration
      : (playback.currentSong.duration ?? 0);
  return (
    <Text numberOfLines={1} style={styles.rightText}>
      {`${formatPlaybackTime(position)}/${formatPlaybackTime(duration)}`}
    </Text>
  );
}

export function Header({
  marqueeTitle = false,
  title,
  showPlaybackTime = false,
}: {
  marqueeTitle?: boolean;
  title: string;
  showPlaybackTime?: boolean;
}) {
  return (
    <View style={styles.header}>
      {marqueeTitle ? (
        <SyncedMarqueeText
          align="left"
          containerStyle={styles.titleMarquee}
          group="mobile-header-song"
          text={title}
          style={styles.title}
        />
      ) : (
        <Text numberOfLines={1} style={styles.title}>
          {title}
        </Text>
      )}
      {showPlaybackTime ? <HeaderPlaybackTime /> : null}
    </View>
  );
}

const styles = StyleSheet.create({
  header: {
    height: 28,
    alignItems: "center",
    backgroundColor: "#ecece4",
    borderBottomWidth: 2,
    borderColor: "#9f9f96",
    flexDirection: "row",
    paddingHorizontal: 8,
  },
  title: {
    color: colors.text,
    flex: 1,
    fontSize: 14,
    fontWeight: "700",
    height: 18,
    lineHeight: 18,
  },
  titleMarquee: {
    alignSelf: "center",
    flex: 1,
    height: 18,
    minWidth: 0,
  },
  rightText: {
    color: colors.muted,
    flexShrink: 0,
    fontSize: 12,
    fontVariant: ["tabular-nums"],
    fontWeight: "700",
    marginLeft: 8,
  },
});
