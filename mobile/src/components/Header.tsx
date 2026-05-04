import { StyleSheet, Text, View } from "react-native";

import { SyncedMarqueeText } from "@/components/SyncedMarqueeText";
import { colors } from "@/components/theme";

export function Header({
  marqueeTitle = false,
  title,
  rightText,
}: {
  marqueeTitle?: boolean;
  title: string;
  rightText?: string;
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
      {rightText ? (
        <Text numberOfLines={1} style={styles.rightText}>
          {rightText}
        </Text>
      ) : null}
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
