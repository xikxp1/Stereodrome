import { useEffect, useMemo, useRef, useState } from "react";
import {
  Animated,
  StyleProp,
  StyleSheet,
  Text,
  TextStyle,
  View,
  ViewStyle,
  AppState,
} from "react-native";

type MarqueeMember = {
  maxOffset: number;
  updateOffset(offset: number): void;
};

type MarqueeGroup = {
  state: {
    offset: number;
    direction: "left" | "right";
    maxOffset: number;
  };
  members: Set<MarqueeMember>;
  animationId: number | null;
  pauseTimeout: ReturnType<typeof setTimeout> | null;
};

type SyncedMarqueeTextProps = {
  text: string;
  group: string;
  align?: "center" | "left";
  style?: StyleProp<TextStyle>;
  containerStyle?: StyleProp<ViewStyle>;
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
      state: { offset: 0, direction: "left", maxOffset: 0 },
      members: new Set(),
      animationId: null,
      pauseTimeout: null,
    };
    groupStates.set(groupId, group);
  }
  return group;
}

function stopGroupAnimation(groupId: string) {
  const group = groupStates.get(groupId);
  if (!group) return;

  if (group.animationId !== null) {
    cancelAnimationFrame(group.animationId);
    group.animationId = null;
  }
  if (group.pauseTimeout !== null) {
    clearTimeout(group.pauseTimeout);
    group.pauseTimeout = null;
  }
}

function getGroupMaxOffset(group: MarqueeGroup) {
  let maxOffset = 0;
  group.members.forEach((member) => {
    maxOffset = Math.max(maxOffset, member.maxOffset);
  });
  return maxOffset;
}

function setMemberOffsets(group: MarqueeGroup, offset: number) {
  const progress =
    group.state.maxOffset > 0 ? offset / group.state.maxOffset : 0;
  group.members.forEach((member) => {
    member.updateOffset(Math.max(0, progress * member.maxOffset));
  });
}

function startGroupAnimation(groupId: string) {
  const group = groupStates.get(groupId);
  if (!group || group.members.size === 0 || marqueeAnimationsPaused) return;

  const initialMaxOffset = getGroupMaxOffset(group);
  if (initialMaxOffset <= 0) return;

  group.state.maxOffset = initialMaxOffset;
  group.state.offset = 0;
  group.state.direction = "left";
  setMemberOffsets(group, 0);

  let lastTime: number | null = null;

  function animate(currentTime: number) {
    const group = groupStates.get(groupId);
    if (!group || group.members.size === 0) return;

    const groupMaxOffset = getGroupMaxOffset(group);
    if (groupMaxOffset <= 0) return;
    group.state.maxOffset = groupMaxOffset;

    if (lastTime === null) {
      lastTime = currentTime;
      group.animationId = requestAnimationFrame(animate);
      return;
    }

    const deltaTime = (currentTime - lastTime) / 1000;
    lastTime = currentTime;
    const movement = scrollSpeed * deltaTime;

    if (group.state.direction === "left") {
      group.state.offset += movement;
      if (group.state.offset >= group.state.maxOffset) {
        group.state.offset = group.state.maxOffset;
        group.state.direction = "right";
        lastTime = null;
        setMemberOffsets(group, group.state.maxOffset);
        group.pauseTimeout = setTimeout(() => {
          if (group.members.size > 0) {
            group.animationId = requestAnimationFrame(animate);
          }
        }, pauseDuration);
        return;
      }
    } else {
      group.state.offset -= movement;
      if (group.state.offset <= 0) {
        group.state.offset = 0;
        group.state.direction = "left";
        lastTime = null;
        setMemberOffsets(group, 0);
        group.pauseTimeout = setTimeout(() => {
          if (group.members.size > 0) {
            group.animationId = requestAnimationFrame(animate);
          }
        }, pauseDuration);
        return;
      }
    }

    setMemberOffsets(group, group.state.offset);
    group.animationId = requestAnimationFrame(animate);
  }

  group.pauseTimeout = setTimeout(() => {
    if (marqueeAnimationsPaused) return;
    group.animationId = requestAnimationFrame(animate);
  }, pauseDuration);
}

function restartGroupAnimation(groupId: string) {
  const existingTimeout = restartTimeouts.get(groupId);
  if (existingTimeout) {
    clearTimeout(existingTimeout);
  }

  restartTimeouts.set(
    groupId,
    setTimeout(() => {
      restartTimeouts.delete(groupId);
      stopGroupAnimation(groupId);

      const group = groupStates.get(groupId);
      if (group) {
        group.state.offset = 0;
        group.state.direction = "left";
        group.members.forEach((member) => member.updateOffset(0));
      }

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

export function SyncedMarqueeText({
  align = "center",
  text,
  group,
  style,
  containerStyle,
}: SyncedMarqueeTextProps) {
  const translateX = useRef(new Animated.Value(0)).current;
  const memberRef = useRef<MarqueeMember | null>(null);
  const [containerWidth, setContainerWidth] = useState(0);
  const [textWidth, setTextWidth] = useState(0);
  const maxOffset = useMemo(
    () => Math.max(0, textWidth - containerWidth),
    [containerWidth, textWidth]
  );

  useEffect(() => {
    setTextWidth(0);
    translateX.setValue(0);
  }, [text, translateX]);

  function measureTextWidth(width: number) {
    setTextWidth((existing) => Math.max(existing, Math.ceil(width)));
  }

  useEffect(() => {
    const marqueeGroup = getGroup(group);
    const member: MarqueeMember = {
      maxOffset,
      updateOffset: (offset) => translateX.setValue(-offset),
    };

    memberRef.current = member;
    marqueeGroup.members.add(member);
    restartGroupAnimation(group);

    return () => {
      marqueeGroup.members.delete(member);
      memberRef.current = null;

      if (marqueeGroup.members.size === 0) {
        stopGroupAnimation(group);
        groupStates.delete(group);
      } else {
        restartGroupAnimation(group);
      }
    };
  }, [group, maxOffset, translateX]);

  useEffect(() => {
    if (memberRef.current) {
      memberRef.current.maxOffset = maxOffset;
      memberRef.current.updateOffset(0);
      restartGroupAnimation(group);
    }
  }, [group, maxOffset, text]);

  return (
    <View
      onLayout={(event) => setContainerWidth(event.nativeEvent.layout.width)}
      style={[styles.container, containerStyle]}
    >
      <View pointerEvents="none" style={styles.measureBox}>
        <Text
          numberOfLines={1}
          onLayout={(event) => measureTextWidth(event.nativeEvent.layout.width)}
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
