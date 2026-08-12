import { useRef } from "react";
import { Platform, Pressable, StyleSheet, Text, View } from "react-native";
import { GestureDetector, usePanGesture } from "react-native-gesture-handler";
import { FastForward, Menu, Pause, Play, Rewind } from "lucide-react-native";
import * as Haptics from "expo-haptics";

import { colors } from "@/components/theme";
import { useInputBus } from "@/context/InputContext";
import { usePlaybackMetadata } from "@/core/selectors";

const hapticTickIntervalMs = 45;
let lastHapticTick = 0;

function tick() {
  const now = Date.now();
  if (now - lastHapticTick < hapticTickIntervalMs) {
    return;
  }

  lastHapticTick = now;
  if (Platform.OS === "android") {
    void Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Clock_Tick);
    return;
  }

  void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
}

function confirm() {
  if (Platform.OS === "android") {
    void Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Virtual_Key);
    return;
  }

  void Haptics.selectionAsync();
}

export function ClickWheel() {
  const { emit } = useInputBus();
  const playback = usePlaybackMetadata();
  const lastAngle = useRef<number | null>(null);
  const centerLongPressed = useRef(false);
  const menuLongPressed = useRef(false);
  const hasNext = playback.nextSong !== null;
  const TransportIcon = playback.isPlaying ? Pause : Play;

  const pan = usePanGesture({
    runOnJS: true,
    onBegin: (event) => {
      lastAngle.current = Math.atan2(event.y - 110, event.x - 110);
    },
    onUpdate: (event) => {
      const nextAngle = Math.atan2(event.y - 110, event.x - 110);
      const previous = lastAngle.current;
      if (previous === null) {
        lastAngle.current = nextAngle;
        return;
      }
      const delta = nextAngle - previous;
      if (Math.abs(delta) > 0.22) {
        tick();
        emit(delta > 0 ? "scroll_forward" : "scroll_backward");
        lastAngle.current = nextAngle;
      }
    },
    onFinalize: () => {
      lastAngle.current = null;
    },
  });

  return (
    <GestureDetector gesture={pan}>
      <View style={styles.wheel}>
        <Pressable
          accessibilityLabel="Menu"
          delayLongPress={450}
          onLongPress={() => {
            menuLongPressed.current = true;
            confirm();
            emit("menu_long");
          }}
          onPress={() => {
            if (menuLongPressed.current) {
              menuLongPressed.current = false;
              return;
            }
            confirm();
            emit("menu");
          }}
          style={[styles.button, styles.menu]}
        >
          <Menu color={colors.wheelIcon} size={28} />
        </Pressable>
        <Pressable
          accessibilityLabel="Previous"
          onPress={() => {
            confirm();
            emit("previous");
          }}
          style={[styles.button, styles.previous]}
        >
          <Rewind color={colors.wheelIcon} size={28} />
        </Pressable>
        <Pressable
          accessibilityLabel="Next"
          accessibilityState={{ disabled: !hasNext }}
          disabled={!hasNext}
          onPress={() => {
            confirm();
            emit("next");
          }}
          style={[styles.button, styles.next, !hasNext && styles.disabled]}
        >
          <FastForward
            color={hasNext ? colors.wheelIcon : colors.muted}
            size={28}
          />
        </Pressable>
        <Pressable
          accessibilityLabel={playback.isPlaying ? "Pause" : "Play"}
          onPress={() => {
            confirm();
            emit("play_pause");
          }}
          style={[styles.button, styles.play]}
        >
          <TransportIcon color={colors.wheelIcon} size={24} />
        </Pressable>
        <Pressable
          accessibilityLabel="Select"
          delayLongPress={450}
          onLongPress={() => {
            centerLongPressed.current = true;
            confirm();
            emit("select_long");
          }}
          onPress={() => {
            if (centerLongPressed.current) {
              centerLongPressed.current = false;
              return;
            }
            confirm();
            emit("select");
          }}
          style={styles.center}
        >
          <Text style={styles.centerText}> </Text>
        </Pressable>
      </View>
    </GestureDetector>
  );
}

const styles = StyleSheet.create({
  wheel: {
    width: 220,
    height: 220,
    alignItems: "center",
    alignSelf: "center",
    backgroundColor: colors.wheel,
    borderColor: "#b9b9b2",
    borderRadius: 110,
    borderWidth: 1,
    justifyContent: "center",
    position: "relative",
  },
  button: {
    alignItems: "center",
    height: 52,
    justifyContent: "center",
    position: "absolute",
    width: 64,
  },
  menu: {
    top: 10,
  },
  previous: {
    left: 10,
  },
  next: {
    right: 10,
  },
  play: {
    bottom: 10,
  },
  disabled: {
    opacity: 0.34,
  },
  center: {
    width: 84,
    height: 84,
    backgroundColor: colors.center,
    borderColor: "#b9b9b2",
    borderRadius: 42,
    borderWidth: 1,
  },
  centerText: {
    fontSize: 1,
  },
});
