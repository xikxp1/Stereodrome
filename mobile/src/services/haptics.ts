import * as Haptics from "expo-haptics";
import { Platform } from "react-native";

const tickIntervalMs = 45;
let lastTick = 0;

function perform(feedback: Promise<void>): void {
  void feedback.catch(() => undefined);
}

function tick(): void {
  const now = Date.now();
  if (now - lastTick < tickIntervalMs) {
    return;
  }

  lastTick = now;
  if (Platform.OS === "android") {
    perform(
      Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Clock_Tick)
    );
    return;
  }
  perform(Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light));
}

function selection(): void {
  if (Platform.OS === "android") {
    perform(
      Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Virtual_Key)
    );
    return;
  }
  perform(Haptics.selectionAsync());
}

function emphasis(): void {
  if (Platform.OS === "android") {
    perform(
      Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Long_Press)
    );
    return;
  }
  perform(Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium));
}

function warning(): void {
  if (Platform.OS === "android") {
    perform(Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Confirm));
    return;
  }
  perform(Haptics.notificationAsync(Haptics.NotificationFeedbackType.Warning));
}

function toggle(enabled: boolean): void {
  if (Platform.OS === "android") {
    perform(
      Haptics.performAndroidHapticsAsync(
        enabled
          ? Haptics.AndroidHaptics.Toggle_On
          : Haptics.AndroidHaptics.Toggle_Off
      )
    );
    return;
  }
  perform(Haptics.selectionAsync());
}

export const haptics = { emphasis, selection, tick, toggle, warning };
