import { Animated, Easing, Pressable, StyleSheet, View } from "react-native";
import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { RefreshCw, Repeat2, Shuffle } from "lucide-react-native";

import { ClickWheel } from "@/components/ClickWheel";
import { Header } from "@/components/Header";
import { colors } from "@/components/theme";
import { useInputBus } from "@/context/InputContext";
import { useMobileSettings } from "@/context/MobileSettingsContext";
import { usePlayback } from "@/context/PlaybackContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { renderView } from "@/screens/renderView";

function formatPlaybackTime(seconds: number) {
  const safeSeconds = Math.max(0, Math.floor(seconds));
  const minutes = Math.floor(safeSeconds / 60);
  const remainingSeconds = safeSeconds % 60;
  return `${minutes}:${remainingSeconds.toString().padStart(2, "0")}`;
}

export function IpodShell() {
  const view = useViewStack();
  const navigationProgress = useRef(new Animated.Value(1)).current;
  const previousTransitionKey = useRef(view.transitionKey);
  const { subscribe } = useInputBus();
  const { buttonHandedness } = useMobileSettings();
  const playback = usePlayback();
  const stereodrome = useStereodrome();
  const insets = useSafeAreaInsets();
  const current = stereodrome.status.connected
    ? view.current
    : { name: "connect" as const, title: "Connect" };
  const navigationOffset = view.transitionDirection === "back" ? -24 : 24;
  const leftHandedButtons = buttonHandedness === "left";
  const playbackDuration =
    playback.duration || playback.currentSong?.duration || 0;
  const playbackTime = playback.currentSong
    ? `${formatPlaybackTime(playback.position)}/${formatPlaybackTime(
        playbackDuration
      )}`
    : undefined;
  const headerTitle =
    current.name !== "nowPlaying" && playback.currentSong
      ? `${playback.currentSong.artist ?? "Unknown Artist"} - ${
          playback.currentSong.title
        }`
      : current.title;
  const screenStyle = useMemo(
    () => ({
      opacity: navigationProgress.interpolate({
        inputRange: [0, 1],
        outputRange: [0.72, 1],
      }),
      transform: [
        {
          translateX: navigationProgress.interpolate({
            inputRange: [0, 1],
            outputRange: [
              view.transitionDirection === "replace" ? 0 : navigationOffset,
              0,
            ],
          }),
        },
      ],
    }),
    [
      navigationOffset,
      navigationProgress,
      view.transitionDirection,
      view.transitionKey,
    ]
  );

  useLayoutEffect(() => {
    if (previousTransitionKey.current === view.transitionKey) {
      return;
    }

    previousTransitionKey.current = view.transitionKey;
    navigationProgress.stopAnimation();
    navigationProgress.setValue(0);
    Animated.timing(navigationProgress, {
      duration: 180,
      easing: Easing.out(Easing.cubic),
      toValue: 1,
      useNativeDriver: true,
    }).start();
  }, [navigationProgress, view.transitionKey]);

  useEffect(
    () =>
      subscribe((input) => {
        if (input === "menu") view.pop();
        if (input === "menu_long" && playback.currentSong) {
          view.showNowPlaying();
        }
        if (input === "play_pause") void playback.toggle();
        if (input === "next") void playback.next();
        if (input === "previous") void playback.previous();
        if (current.name === "nowPlaying" && input === "scroll_forward") {
          void playback.seekBy(5);
        }
        if (current.name === "nowPlaying" && input === "scroll_backward") {
          void playback.seekBy(-5);
        }
      }),
    [current.name, playback, subscribe, view]
  );

  return (
    <View style={styles.stage}>
      <View
        style={[styles.shell, { paddingTop: Math.max(14, insets.top + 10) }]}
      >
        <View style={styles.screen}>
          <Header
            marqueeTitle={
              current.name !== "nowPlaying" && !!playback.currentSong
            }
            rightText={playbackTime}
            title={headerTitle}
          />
          <View style={styles.viewport}>
            <Animated.View style={[styles.animatedViewport, screenStyle]}>
              {renderView(current)}
            </Animated.View>
          </View>
        </View>
        <View style={styles.wheelSlot}>
          <Pressable
            accessibilityLabel="Toggle repeat"
            accessibilityRole="button"
            accessibilityState={{ selected: playback.repeatEnabled }}
            onPress={() => void playback.toggleRepeat()}
            style={[
              styles.queueButton,
              leftHandedButtons ? styles.repeatLeft : styles.repeatRight,
              playback.repeatEnabled && styles.queueButtonActive,
            ]}
          >
            <Repeat2
              color={
                playback.repeatEnabled ? colors.selectedText : colors.wheelIcon
              }
              size={20}
            />
          </Pressable>
          <View
            style={[
              styles.queueButtonGroup,
              leftHandedButtons ? styles.groupRight : styles.groupLeft,
            ]}
          >
            <Pressable
              accessibilityLabel="Toggle shuffle"
              accessibilityRole="button"
              accessibilityState={{ selected: playback.shuffleEnabled }}
              onPress={() => void playback.toggleShuffle()}
              style={[
                styles.queueButton,
                playback.shuffleEnabled && styles.queueButtonActive,
              ]}
            >
              <Shuffle
                color={
                  playback.shuffleEnabled
                    ? colors.selectedText
                    : colors.wheelIcon
                }
                size={20}
              />
            </Pressable>
            <Pressable
              accessibilityLabel="Reroll next track"
              accessibilityRole="button"
              onPress={() => void playback.rerollNext()}
              style={styles.queueButton}
            >
              <RefreshCw color={colors.wheelIcon} size={20} />
            </Pressable>
          </View>
          <ClickWheel />
        </View>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  stage: {
    flex: 1,
    backgroundColor: colors.shell,
  },
  shell: {
    alignItems: "center",
    backgroundColor: colors.shell,
    flex: 1,
    gap: 16,
    paddingBottom: 14,
    paddingHorizontal: 18,
    width: "100%",
  },
  screen: {
    backgroundColor: colors.screen,
    borderColor: colors.screenBorder,
    borderRadius: 8,
    borderWidth: 4,
    flex: 1,
    minHeight: 160,
    overflow: "hidden",
    width: "100%",
  },
  viewport: {
    flex: 1,
    overflow: "hidden",
  },
  animatedViewport: {
    flex: 1,
  },
  wheelSlot: {
    alignItems: "center",
    flexShrink: 0,
    height: 244,
    justifyContent: "center",
    position: "relative",
    width: "100%",
  },
  queueButton: {
    alignItems: "center",
    backgroundColor: "rgba(248, 248, 244, 0.86)",
    borderColor: "#b9b9b2",
    borderRadius: 18,
    borderWidth: 1,
    height: 36,
    justifyContent: "center",
    width: 36,
  },
  queueButtonActive: {
    backgroundColor: colors.selected,
    borderColor: colors.selected,
  },
  repeatLeft: {
    left: 8,
    position: "absolute",
    top: 36,
  },
  repeatRight: {
    position: "absolute",
    right: 8,
    top: 36,
  },
  queueButtonGroup: {
    gap: 12,
    position: "absolute",
    top: 36,
  },
  groupLeft: {
    left: 8,
  },
  groupRight: {
    right: 8,
  },
});
