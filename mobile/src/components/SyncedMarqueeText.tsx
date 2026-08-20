import { useEffect, useMemo, useState } from "react";
import {
  Animated,
  AppState,
  Easing,
  type StyleProp,
  StyleSheet,
  Text,
  type TextStyle,
  View,
  type ViewStyle,
} from "react-native";

type MarqueeMember = {
  maxOffset: number;
};

type MarqueeGroup = {
  progress: Animated.Value;
  members: Set<MarqueeMember>;
  animation: Animated.CompositeAnimation | null;
};

const scrollSpeed = 30;
const pauseDuration = 2000;
const restartDelay = 50;
const groupStates = new Map<string, MarqueeGroup>();
const restartTimeouts = new Map<string, ReturnType<typeof setTimeout>>();
let marqueeAnimationsPaused = AppState.currentState !== "active";

function getGroup(groupId: string) {
  let group = groupStates.get(groupId);
  if (!group) {
    group = {
      progress: new Animated.Value(0),
      members: new Set(),
      animation: null,
    };
    groupStates.set(groupId, group);
  }
  return group;
}

function getGroupMaxOffset(group: MarqueeGroup) {
  let maxOffset = 0;
  group.members.forEach((member) => {
    maxOffset = Math.max(maxOffset, member.maxOffset);
  });
  return maxOffset;
}

function stopGroupAnimation(groupId: string) {
  const group = groupStates.get(groupId);
  if (!group) {
    return;
  }

  if (group.animation !== null) {
    group.animation.stop();
    group.animation = null;
  }
  group.progress.setValue(0);
}

function startGroupAnimation(groupId: string) {
  const group = groupStates.get(groupId);
  if (!group || group.members.size === 0 || marqueeAnimationsPaused) {
    return;
  }

  const maxOffset = getGroupMaxOffset(group);
  if (maxOffset <= 0) {
    return;
  }

  // A single native-driven loop per group: members interpolate the shared
  // progress into their own offsets, so no JS code runs per frame. The pace
  // is set by the widest member; narrower members move proportionally slower
  // and finish in lockstep, matching the previous frame-loop behavior.
  const duration = (maxOffset / scrollSpeed) * 1000;
  const animation = Animated.loop(
    Animated.sequence([
      Animated.delay(pauseDuration),
      Animated.timing(group.progress, {
        duration,
        easing: Easing.linear,
        toValue: 1,
        useNativeDriver: true,
      }),
      Animated.delay(pauseDuration),
      Animated.timing(group.progress, {
        duration,
        easing: Easing.linear,
        toValue: 0,
        useNativeDriver: true,
      }),
    ])
  );
  group.animation = animation;
  animation.start();
}

function restartGroupAnimation(groupId: string) {
  const existingTimeout = restartTimeouts.get(groupId);
  if (existingTimeout !== undefined) {
    clearTimeout(existingTimeout);
  }

  restartTimeouts.set(
    groupId,
    setTimeout(() => {
      restartTimeouts.delete(groupId);
      stopGroupAnimation(groupId);
      startGroupAnimation(groupId);
    }, restartDelay)
  );
}

AppState.addEventListener("change", (nextState) => {
  marqueeAnimationsPaused = nextState !== "active";
  groupStates.forEach((_, groupId) => {
    if (marqueeAnimationsPaused) {
      stopGroupAnimation(groupId);
    } else {
      restartGroupAnimation(groupId);
    }
  });
});

type SyncedMarqueeTextProps = {
  text: string;
  group: string;
  align?: "center" | "left";
  style?: StyleProp<TextStyle>;
  containerStyle?: StyleProp<ViewStyle>;
};

export function SyncedMarqueeText({
  align = "center",
  text,
  group,
  style,
  containerStyle,
}: SyncedMarqueeTextProps) {
  const [containerWidth, setContainerWidth] = useState(0);
  const [textMeasurement, setTextMeasurement] = useState({ text, width: 0 });
  const textWidth = textMeasurement.text === text ? textMeasurement.width : 0;
  const maxOffset = useMemo(
    () => Math.max(0, textWidth - containerWidth),
    [containerWidth, textWidth]
  );
  const translateX = useMemo(
    () =>
      getGroup(group).progress.interpolate({
        inputRange: [0, 1],
        outputRange: [0, -maxOffset],
      }),
    [group, maxOffset]
  );

  function measureTextWidth(width: number) {
    setTextMeasurement((existing) => ({
      text,
      width:
        existing.text === text
          ? Math.max(existing.width, Math.ceil(width))
          : Math.ceil(width),
    }));
  }

  useEffect(() => {
    const marqueeGroup = getGroup(group);
    const member: MarqueeMember = { maxOffset };
    marqueeGroup.members.add(member);
    restartGroupAnimation(group);

    return () => {
      marqueeGroup.members.delete(member);

      if (marqueeGroup.members.size === 0) {
        const pendingRestart = restartTimeouts.get(group);
        if (pendingRestart !== undefined) {
          clearTimeout(pendingRestart);
          restartTimeouts.delete(group);
        }
        stopGroupAnimation(group);
        groupStates.delete(group);
      } else {
        restartGroupAnimation(group);
      }
    };
  }, [group, maxOffset]);

  return (
    <View
      onLayout={(event) => {
        setContainerWidth(event.nativeEvent.layout.width);
      }}
      style={[styles.container, containerStyle]}
    >
      <View pointerEvents="none" style={styles.measureBox}>
        <Text
          numberOfLines={1}
          onLayout={(event) => {
            measureTextWidth(event.nativeEvent.layout.width);
          }}
          onTextLayout={(event) => {
            const widestLine = event.nativeEvent.lines.reduce(
              (widest, line) => Math.max(widest, line.width),
              0
            );
            measureTextWidth(widestLine);
          }}
          style={[style, styles.measureText]}
        >
          {text}
        </Text>
      </View>
      <Animated.Text
        numberOfLines={1}
        style={[
          style,
          maxOffset === 0 && align === "center" && styles.staticText,
          {
            transform: [{ translateX }],
            width: maxOffset === 0 ? "100%" : textWidth,
          },
        ]}
      >
        {text}
      </Animated.Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    overflow: "hidden",
    width: "100%",
  },
  measureBox: {
    left: 0,
    opacity: 0,
    position: "absolute",
    top: 0,
    width: 10000,
  },
  measureText: {
    alignSelf: "flex-start",
    flexShrink: 0,
    maxWidth: 10000,
  },
  staticText: {
    textAlign: "center",
  },
});
