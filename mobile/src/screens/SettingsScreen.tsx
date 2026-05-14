import { useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { colors } from "@/components/theme";
import { useMobileSettings } from "@/context/MobileSettingsContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function SettingsScreen() {
  const stereodrome = useStereodrome();
  const mobileSettings = useMobileSettings();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const syncStatus = useQuery({
    queryKey: ["library-sync-status"],
    queryFn: stereodromeCore.getLibrarySyncStatus,
  });
  const cacheStats = useQuery({
    queryKey: ["audio-cache-stats"],
    queryFn: stereodromeCore.getAudioCacheStats,
  });

  async function runSync(mode: "full" | "incremental" | "reconcile") {
    setBusy(true);
    setMessage(null);
    try {
      if (mode === "incremental") {
        await stereodrome.syncIncremental();
      } else if (mode === "reconcile") {
        await stereodrome.reconcile();
      } else {
        await stereodrome.sync();
      }
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
      <Pressable
        disabled={busy}
        onPress={() => runSync("full")}
        style={styles.button}
      >
        {busy ? (
          <ActivityIndicator color="#fff" />
        ) : (
          <Text style={styles.buttonText}>Sync Library</Text>
        )}
      </Pressable>
      <View style={styles.buttonRow}>
        <Pressable
          disabled={busy}
          onPress={() => runSync("incremental")}
          style={styles.secondaryButton}
        >
          <Text style={styles.secondaryButtonText}>Incremental</Text>
        </Pressable>
        <Pressable
          disabled={busy}
          onPress={() => runSync("reconcile")}
          style={styles.secondaryButton}
        >
          <Text style={styles.secondaryButtonText}>Reconcile</Text>
        </Pressable>
      </View>
      <Text style={styles.copy}>
        Last success:{" "}
        {syncStatus.data?.incremental.last_success_at ??
          syncStatus.data?.full_reconcile.last_success_at ??
          "Never"}
      </Text>
      {syncStatus.data?.incremental.last_error ? (
        <Text style={styles.error}>
          {syncStatus.data.incremental.last_error}
        </Text>
      ) : null}
      {message ? <Text style={styles.copy}>{message}</Text> : null}
      <Text style={styles.heading}>Cache</Text>
      <Text style={styles.copy}>
        {formatBytes(cacheStats.data?.total_size ?? 0)} cached across{" "}
        {cacheStats.data?.file_count ?? 0} files
      </Text>
      <Pressable
        disabled={busy}
        onPress={async () => {
          setBusy(true);
          try {
            await stereodromeCore.clearAudioCache();
            await queryClient.invalidateQueries({
              queryKey: ["audio-cache-stats"],
            });
            setMessage("Cache cleared");
          } catch (e) {
            setMessage(e instanceof Error ? e.message : String(e));
          } finally {
            setBusy(false);
          }
        }}
        style={styles.secondaryButton}
      >
        <Text style={styles.secondaryButtonText}>Clear Audio Cache</Text>
      </Pressable>
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
  buttonRow: {
    flexDirection: "row",
    gap: 8,
    marginBottom: 12,
  },
  secondaryButton: {
    alignItems: "center",
    borderColor: colors.screenBorder,
    borderRadius: 4,
    borderWidth: 2,
    flex: 1,
    height: 32,
    justifyContent: "center",
  },
  secondaryButtonText: {
    color: colors.text,
    fontSize: 12,
    fontWeight: "800",
  },
  error: {
    color: "#ef4444",
    fontSize: 12,
    fontWeight: "700",
    marginBottom: 12,
  },
  buttonText: {
    color: "#fff",
    fontWeight: "800",
  },
});

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KB`;
  }
  if (bytes < 1024 * 1024 * 1024) {
    return `${Math.round(bytes / (1024 * 1024))} MB`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}
