import { useCallback, useEffect, useRef, useState } from "react";
import {
  Animated,
  Easing,
  Pressable,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { CircleAlert, X } from "lucide-react-native";

import { haptics } from "@/services/haptics";

type ErrorToastProps = {
  message: string | null;
  onDismiss: () => void;
};

const visibleDuration = 6500;

export function ErrorToast({ message, onDismiss }: ErrorToastProps) {
  const progress = useRef(new Animated.Value(0)).current;
  const dismissTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [displayedMessage, setDisplayedMessage] = useState<string | null>(null);

  const clearDismissTimer = useCallback(() => {
    if (dismissTimer.current !== null) {
      clearTimeout(dismissTimer.current);
      dismissTimer.current = null;
    }
  }, []);

  const hide = useCallback(
    (clearError: boolean) => {
      clearDismissTimer();
      progress.stopAnimation();
      Animated.timing(progress, {
        duration: 180,
        easing: Easing.in(Easing.cubic),
        toValue: 0,
        useNativeDriver: true,
      }).start(({ finished }) => {
        if (finished) {
          setDisplayedMessage(null);
          if (clearError) {
            onDismiss();
          }
        }
      });
    },
    [clearDismissTimer, onDismiss, progress]
  );

  const dismiss = useCallback(() => {
    hide(true);
  }, [hide]);

  useEffect(() => {
    const nextMessage = message?.trim() ?? "";
    if (nextMessage.length === 0) {
      hide(false);
      return clearDismissTimer;
    }

    clearDismissTimer();
    progress.stopAnimation();
    progress.setValue(0);
    setDisplayedMessage(nextMessage);
    Animated.spring(progress, {
      damping: 18,
      mass: 0.7,
      stiffness: 220,
      toValue: 1,
      useNativeDriver: true,
    }).start();
    dismissTimer.current = setTimeout(dismiss, visibleDuration);

    return clearDismissTimer;
  }, [clearDismissTimer, dismiss, hide, message, progress]);

  useEffect(
    () => () => {
      clearDismissTimer();
      progress.stopAnimation();
    },
    [clearDismissTimer, progress]
  );

  if (displayedMessage === null) {
    return null;
  }

  return (
    <Animated.View
      accessibilityLiveRegion="assertive"
      accessibilityRole="alert"
      style={[
        styles.positioner,
        {
          opacity: progress,
          transform: [
            {
              translateY: progress.interpolate({
                inputRange: [0, 1],
                outputRange: [-14, 0],
              }),
            },
            {
              scale: progress.interpolate({
                inputRange: [0, 1],
                outputRange: [0.98, 1],
              }),
            },
          ],
        },
      ]}
    >
      <Pressable
        accessibilityHint="Dismisses this notification"
        accessibilityLabel={`Error: ${displayedMessage}`}
        accessibilityRole="button"
        onPress={() => {
          haptics.selection();
          dismiss();
        }}
        style={styles.toast}
      >
        <View style={styles.iconBadge}>
          <CircleAlert color={styles.icon.color} size={18} strokeWidth={2.4} />
        </View>
        <View style={styles.copy}>
          <Text style={styles.title}>Something went wrong</Text>
          <Text numberOfLines={3} style={styles.message}>
            {displayedMessage}
          </Text>
        </View>
        <X color={styles.dismissIcon.color} size={16} strokeWidth={2.4} />
      </Pressable>
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  positioner: {
    elevation: 12,
    left: 10,
    position: "absolute",
    right: 10,
    shadowColor: "#000000",
    shadowOffset: { height: 4, width: 0 },
    shadowOpacity: 0.24,
    shadowRadius: 10,
    top: 10,
    zIndex: 20,
  },
  toast: {
    alignItems: "center",
    backgroundColor: "rgba(43, 28, 27, 0.97)",
    borderColor: "rgba(248, 113, 113, 0.42)",
    borderRadius: 10,
    borderWidth: 1,
    flexDirection: "row",
    gap: 10,
    minHeight: 58,
    paddingHorizontal: 12,
    paddingVertical: 10,
  },
  iconBadge: {
    alignItems: "center",
    backgroundColor: "rgba(248, 113, 113, 0.16)",
    borderRadius: 16,
    height: 32,
    justifyContent: "center",
    width: 32,
  },
  icon: {
    color: "#f87171",
  },
  copy: {
    flex: 1,
    minWidth: 0,
  },
  title: {
    color: "#fff7f6",
    fontSize: 12,
    fontWeight: "800",
    letterSpacing: 0.1,
  },
  message: {
    color: "#efcfcb",
    fontSize: 11,
    fontWeight: "600",
    lineHeight: 15,
    marginTop: 1,
  },
  dismissIcon: {
    color: "#d9aaa4",
  },
});
