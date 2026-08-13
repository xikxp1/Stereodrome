import {
  Animated,
  Easing,
  Pressable,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import {
  Dices,
  Download,
  LoaderCircle,
  MoreHorizontal,
  Repeat,
  Repeat1,
  Shuffle,
} from "lucide-react-native";

import { ClickWheel } from "@/components/ClickWheel";
import { ErrorToast } from "@/components/ErrorToast";
import { Header } from "@/components/Header";
import { colors } from "@/components/theme";
import { useInputBus } from "@/context/InputContext";
import { useMobileSettings } from "@/context/MobileSettingsContext";
import { useFileState, usePlayback, useStereodrome } from "@/core/selectors";
import { coreActions } from "@/core/store";
import { useSongActions } from "@/context/SongActionContext";
import {
  connectView,
  homeView,
  loadingView,
  useViewStack,
} from "@/context/ViewContext";
import { renderView } from "@/screens/renderView";
import { haptics } from "@/services/haptics";

function ignorePlaybackFailure(promise: Promise<void>): void {
  void promise.catch(() => undefined);
}

export function IpodShell() {
  const view = useViewStack();
  const {
    current,
    pop,
    reset,
    showNowPlaying,
    transitionDirection,
    transitionKey,
  } = view;
  const navigationProgress = useRef(new Animated.Value(1)).current;
  const downloadProgress = useRef(new Animated.Value(0)).current;
  const previousTransitionKey = useRef(transitionKey);
  const { subscribe } = useInputBus();
  const { buttonHandedness } = useMobileSettings();
  const playback = usePlayback();
  const fileState = useFileState();
  const songActions = useSongActions();
  const stereodrome = useStereodrome();
  const insets = useSafeAreaInsets();
  const navigationOffset = transitionDirection === "back" ? -24 : 24;
  const leftHandedButtons = buttonHandedness === "left";
  const downloadQueueSize = fileState.downloadingSongIds.size;
  const downloadsActive = downloadQueueSize > 0;
  const RepeatIcon = playback.repeatMode === "One" ? Repeat1 : Repeat;
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
              transitionDirection === "replace" ? 0 : navigationOffset,
              0,
            ],
          }),
        },
      ],
    }),
    [navigationOffset, navigationProgress, transitionDirection]
  );

  useLayoutEffect(() => {
    if (previousTransitionKey.current === transitionKey) {
      return;
    }

    previousTransitionKey.current = transitionKey;
    navigationProgress.stopAnimation();
    navigationProgress.setValue(0);
    Animated.timing(navigationProgress, {
      duration: 180,
      easing: Easing.out(Easing.cubic),
      toValue: 1,
      useNativeDriver: true,
    }).start();
  }, [navigationProgress, transitionKey]);

  useEffect(() => {
    downloadProgress.stopAnimation();
    downloadProgress.setValue(0);
    if (!downloadsActive) {
      return undefined;
    }

    const animation = Animated.loop(
      Animated.timing(downloadProgress, {
        duration: 1100,
        easing: Easing.linear,
        toValue: 1,
        useNativeDriver: true,
      })
    );
    animation.start();
    return () => {
      animation.stop();
    };
  }, [downloadProgress, downloadsActive]);

  useEffect(() => {
    if (!stereodrome.ready) {
      if (current.name !== loadingView.name) {
        reset(loadingView);
      }
      return;
    }

    if (!stereodrome.hasConfiguredServer) {
      if (current.name !== connectView.name) {
        reset(connectView);
      }
      return;
    }

    if (
      current.name === loadingView.name ||
      current.name === connectView.name
    ) {
      reset(homeView);
    }
  }, [stereodrome.hasConfiguredServer, stereodrome.ready, current.name, reset]);

  useEffect(
    () =>
      subscribe((input) => {
        if (!stereodrome.ready) {
          return;
        }
        if (input === "menu") {
          pop();
        }
        if (input === "menu_long" && playback.currentSong) {
          if (current.name === "nowPlaying") {
            songActions.openSongContextMenu();
          } else {
            showNowPlaying();
          }
        }
        if (input === "play_pause") {
          ignorePlaybackFailure(playback.toggle());
        }
        if (input === "next") {
          ignorePlaybackFailure(playback.next());
        }
        if (input === "previous") {
          ignorePlaybackFailure(playback.previous());
        }
        if (current.name === "nowPlaying" && input === "scroll_forward") {
          ignorePlaybackFailure(playback.seekBy(5));
        }
        if (current.name === "nowPlaying" && input === "scroll_backward") {
          ignorePlaybackFailure(playback.seekBy(-5));
        }
      }),
    [
      current.name,
      playback,
      pop,
      showNowPlaying,
      songActions,
      stereodrome.ready,
      subscribe,
    ]
  );

  return (
    <View style={styles.stage}>
      <View
        style={[styles.shell, { paddingTop: Math.max(14, insets.top + 10) }]}
      >
        <View style={styles.screen}>
          <Header
            marqueeTitle={
              current.name !== "nowPlaying" && Boolean(playback.currentSong)
            }
            showPlaybackTime={Boolean(playback.currentSong)}
            title={headerTitle}
          />
          <View style={styles.viewport}>
            <Animated.View style={[styles.animatedViewport, screenStyle]}>
              {renderView(current)}
            </Animated.View>
            <ErrorToast
              message={playback.error}
              onDismiss={coreActions.dismissError}
            />
          </View>
        </View>
        <View style={styles.wheelSlot}>
          <Pressable
            accessibilityLabel="Cycle repeat mode"
            accessibilityRole="button"
            accessibilityState={{ selected: playback.repeatEnabled }}
            onPress={() => {
              haptics.toggle(!playback.repeatEnabled);
              ignorePlaybackFailure(playback.toggleRepeat());
            }}
            style={[
              styles.queueButton,
              leftHandedButtons ? styles.repeatLeft : styles.repeatRight,
              playback.repeatEnabled && styles.queueButtonActive,
            ]}
          >
            <RepeatIcon
              color={
                playback.repeatEnabled ? colors.selectedText : colors.wheelIcon
              }
              size={20}
            />
          </Pressable>
          <Pressable
            accessibilityLabel="Song actions"
            accessibilityRole="button"
            accessibilityState={{
              disabled: !songActions.canOpenSongContextMenu,
            }}
            disabled={!songActions.canOpenSongContextMenu}
            onPress={() => {
              haptics.selection();
              songActions.openSongContextMenu();
            }}
            style={[
              styles.queueButton,
              leftHandedButtons
                ? styles.songActionsLeft
                : styles.songActionsRight,
              !songActions.canOpenSongContextMenu && styles.queueButtonDisabled,
            ]}
          >
            <MoreHorizontal color={colors.wheelIcon} size={20} />
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
              onPress={() => {
                haptics.toggle(!playback.shuffleEnabled);
                ignorePlaybackFailure(playback.toggleShuffle());
              }}
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
              onPress={() => {
                haptics.selection();
                ignorePlaybackFailure(playback.rerollNext());
              }}
              style={styles.queueButton}
            >
              <Dices color={colors.wheelIcon} size={20} />
            </Pressable>
            <Pressable
              accessibilityLabel={`Downloads, ${downloadQueueSize} queued`}
              accessibilityRole="button"
              accessibilityState={{ busy: downloadsActive }}
              onPress={() => {
                haptics.selection();
                if (current.name !== "downloads") {
                  view.push({ name: "downloads", title: "Downloads" });
                }
              }}
              style={styles.queueButton}
            >
              {downloadsActive ? (
                <Animated.View
                  style={{
                    transform: [
                      {
                        rotate: downloadProgress.interpolate({
                          inputRange: [0, 1],
                          outputRange: ["0deg", "360deg"],
                        }),
                      },
                    ],
                  }}
                >
                  <LoaderCircle color="#2563eb" size={20} />
                </Animated.View>
              ) : (
                <Download color={colors.wheelIcon} size={20} />
              )}
              <View
                style={[
                  styles.downloadCount,
                  downloadsActive && styles.downloadCountActive,
                ]}
              >
                <Text style={styles.downloadCountText}>
                  {downloadQueueSize.toLocaleString()}
                </Text>
              </View>
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
  queueButtonDisabled: {
    opacity: 0.42,
  },
  downloadCount: {
    alignItems: "center",
    backgroundColor: colors.muted,
    borderColor: colors.wheel,
    borderRadius: 8,
    borderWidth: 1,
    bottom: -4,
    justifyContent: "center",
    minWidth: 17,
    paddingHorizontal: 3,
    position: "absolute",
    right: -5,
  },
  downloadCountActive: {
    backgroundColor: "#2563eb",
  },
  downloadCountText: {
    color: colors.selectedText,
    fontSize: 9,
    fontWeight: "900",
    lineHeight: 14,
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
  songActionsLeft: {
    left: 8,
    position: "absolute",
    top: 84,
  },
  songActionsRight: {
    position: "absolute",
    right: 8,
    top: 84,
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
