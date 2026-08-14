import { useRef } from "react";
import { Pressable, StyleSheet, Text, View } from "react-native";
import { Gesture, GestureDetector } from "react-native-gesture-handler";
import { FastForward, Menu, Pause, Play, Rewind } from "lucide-react-native";

import { colors } from "@/components/theme";
import { useInputBus } from "@/context/InputContext";
import { usePlaybackMetadata } from "@/core/selectors";
import { haptics } from "@/services/haptics";

export function ClickWheel() {
  const { emit } = useInputBus();
  const playback = usePlaybackMetadata();
  const lastAngle = useRef<number | null>(null);
  const centerLongPressed = useRef(false);
  const menuLongPressed = useRef(false);
  const hasNext = playback.nextSong !== null;
  const TransportIcon = playback.isPlaying ? Pause : Play;

  const pan = Gesture.Pan()
    .runOnJS(true)
    .onBegin((event) => {
      lastAngle.current = Math.atan2(event.y - 110, event.x - 110);
    })
    .onUpdate((event) => {
      const nextAngle = Math.atan2(event.y - 110, event.x - 110);
      const previous = lastAngle.current;
      if (previous === null) {
        lastAngle.current = nextAngle;
        return;
      }
      const rawDelta = nextAngle - previous;
      const delta = Math.atan2(Math.sin(rawDelta), Math.cos(rawDelta));
      if (Math.abs(delta) > 0.22) {
        haptics.tick();
        emit(delta > 0 ? "scroll_forward" : "scroll_backward");
        lastAngle.current = nextAngle;
      }
    })
    .onFinalize(() => {
      lastAngle.current = null;
    });

  return (
    <GestureDetector gesture={pan}>
      <View style={styles.wheel}>
        <Pressable
          accessibilityLabel="Menu"
          delayLongPress={450}
          onLongPress={() => {
            menuLongPressed.current = true;
            haptics.emphasis();
            emit("menu_long");
          }}
          onPress={() => {
            if (menuLongPressed.current) {
              menuLongPressed.current = false;
              return;
            }
            haptics.selection();
            emit("menu");
          }}
          style={[styles.button, styles.menu]}
        >
          <Menu color={colors.wheelIcon} size={28} />
        </Pressable>
        <Pressable
          accessibilityLabel="Previous"
          onPress={() => {
            haptics.selection();
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
            haptics.selection();
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
            haptics.selection();
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
            haptics.emphasis();
            emit("select_long");
          }}
          onPress={() => {
            if (centerLongPressed.current) {
              centerLongPressed.current = false;
              return;
            }
            haptics.selection();
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
