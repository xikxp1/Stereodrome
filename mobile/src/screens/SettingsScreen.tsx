import { useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { useQueryClient } from "@tanstack/react-query";

import { colors } from "@/components/theme";
import { useMobileSettings } from "@/context/MobileSettingsContext";
import { useStereodrome } from "@/context/StereodromeContext";

export function SettingsScreen() {
  const stereodrome = useStereodrome();
  const mobileSettings = useMobileSettings();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function sync() {
    setBusy(true);
    setMessage(null);
    try {
      await stereodrome.sync();
      await queryClient.invalidateQueries();
      setMessage("Library synced");
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <View style={styles.container}>
      <Text style={styles.heading}>Server</Text>
      <Text style={styles.copy}>
        {stereodrome.status.server_url ?? "Not connected"}
      </Text>
      <Pressable disabled={busy} onPress={sync} style={styles.button}>
        {busy ? (
          <ActivityIndicator color="#fff" />
        ) : (
          <Text style={styles.buttonText}>Sync Library</Text>
        )}
      </Pressable>
      {message ? <Text style={styles.copy}>{message}</Text> : null}
      <Text style={styles.heading}>Controls</Text>
      <View style={styles.segmentedControl}>
        <Pressable
          accessibilityRole="button"
          accessibilityState={{
            selected: mobileSettings.buttonHandedness === "right",
          }}
          onPress={() => mobileSettings.setButtonHandedness("right")}
          style={[
            styles.segment,
            mobileSettings.buttonHandedness === "right" &&
              styles.segmentSelected,
          ]}
        >
          <Text
            style={[
              styles.segmentText,
              mobileSettings.buttonHandedness === "right" &&
                styles.segmentSelectedText,
            ]}
          >
            Right handed
          </Text>
        </Pressable>
        <Pressable
          accessibilityRole="button"
          accessibilityState={{
            selected: mobileSettings.buttonHandedness === "left",
          }}
          onPress={() => mobileSettings.setButtonHandedness("left")}
          style={[
            styles.segment,
            mobileSettings.buttonHandedness === "left" &&
              styles.segmentSelected,
          ]}
        >
          <Text
            style={[
              styles.segmentText,
              mobileSettings.buttonHandedness === "left" &&
                styles.segmentSelectedText,
            ]}
          >
            Left handed
          </Text>
        </Pressable>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    padding: 12,
  },
  heading: {
    color: colors.text,
    fontSize: 16,
    fontWeight: "800",
    marginBottom: 4,
  },
  copy: {
    color: colors.muted,
    fontSize: 12,
    fontWeight: "700",
    marginBottom: 12,
  },
  segmentedControl: {
    borderColor: colors.screenBorder,
    borderRadius: 5,
    borderWidth: 2,
    flexDirection: "row",
    marginBottom: 12,
    overflow: "hidden",
  },
  segment: {
    alignItems: "center",
    flex: 1,
    height: 32,
    justifyContent: "center",
  },
  segmentSelected: {
    backgroundColor: colors.selected,
  },
  segmentText: {
    color: colors.text,
    fontSize: 12,
    fontWeight: "800",
  },
  segmentSelectedText: {
    color: colors.selectedText,
  },
  button: {
    alignItems: "center",
    backgroundColor: colors.selected,
    borderRadius: 4,
    height: 34,
    justifyContent: "center",
    marginBottom: 12,
  },
  buttonText: {
    color: "#fff",
    fontWeight: "800",
  },
});
