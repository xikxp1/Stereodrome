import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import {
  type KeyboardTypeOptions,
  Linking,
  Modal,
  Pressable,
  Text,
  TextInput,
  View,
} from "react-native";

import {
  SelectableList,
  type SelectableOption,
} from "@/components/SelectableList";
import { useProtectedSelectableAction } from "@/components/protectedSelectableAction";
import { useMobileSettings } from "@/context/MobileSettingsContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { configureLibrarySyncBackgroundTask } from "@/services/librarySyncScheduler";
import { stereodromeCore } from "@/services/stereodromeCore";
import { settingsScreenStyles as styles } from "@/screens/SettingsScreen.styles";
import type {
  AudioProcessingSettings,
  LastfmQueueItem,
  LibrarySyncStatus,
  ScanStatus,
  SyncSettings,
} from "@/types/music";

const lufsPresets = [-18, -16, -14, -12, -10];
const crossfadePresets = [1000, 3000, 5000, 8000, 12000];
const prefetchCountPresets = [1, 2, 3, 5, 10];
const cacheSizePresetsGb = [0.5, 1, 2, 5, 10, 20, 50];
const incrementalSyncIntervals = [5, 15, 30, 60, 120, 360, 720];
const fullReconcileIntervals = [1, 6, 12, 24, 48, 72, 168];
const librarySyncStatusQueryKey = ["library-sync-status"] as const;
const syncSettingsQueryKey = ["sync-settings"] as const;
const scanStatusQueryKey = ["scan-status"] as const;
const lastfmStatusQueryKey = ["lastfm-status"] as const;
const lastfmQueueQueryKey = ["lastfm-queue"] as const;
const eqLabels = [
  "32",
  "64",
  "125",
  "250",
  "500",
  "1k",
  "2k",
  "4k",
  "8k",
  "12k",
  "16k",
  "20k",
] as const;
const eqMinDb = -12;
const eqMaxDb = 12;
const eqStepDb = 0.5;

type TextEditConfig = {
  title: string;
  value: string;
  keyboardType?: KeyboardTypeOptions;
  onSubmit(value: string): Promise<void>;
};

type SettingsCategory =
  | "server"
  | "sync"
  | "lastfm"
  | "interface"
  | "playback"
  | "normalization"
  | "cache";

const settingsCategories: {
  id: SettingsCategory;
  label: string;
  sublabel: string;
}[] = [
  { id: "server", label: "Server", sublabel: "Connection and account" },
  { id: "sync", label: "Library Sync", sublabel: "Sync status and actions" },
  { id: "lastfm", label: "Last.fm", sublabel: "Scrobbling and offline queue" },
  { id: "interface", label: "Interface", sublabel: "Mobile controls" },
  { id: "playback", label: "Playback", sublabel: "Queue and audio effects" },
  {
    id: "normalization",
    label: "Normalization",
    sublabel: "Loudness and dynamics",
  },
  { id: "cache", label: "Audio Cache", sublabel: "Downloaded audio storage" },
];

type EqPresetId =
  | "flat"
  | "bass_boost"
  | "treble_sparkle"
  | "vocal_clarity"
  | "electronic_punch"
  | "acoustic_warm"
  | "late_night"
  | "rock";

type EqPreset = {
  id: EqPresetId;
  label: string;
  bands: number[];
};

const eqPresets: EqPreset[] = [
  {
    id: "flat",
    label: "Flat",
    bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  },
  {
    id: "bass_boost",
    label: "Bass Boost",
    bands: [5, 4, 3, 2, 1, 0, -1, -2, -2, -1, 0, 0],
  },
  {
    id: "treble_sparkle",
    label: "Treble Sparkle",
    bands: [-2, -2, -1, 0, 0, 0, 1, 2, 3, 4, 4, 3],
  },
  {
    id: "vocal_clarity",
    label: "Vocal Clarity",
    bands: [-2, -1, 0, 2, 3, 3, 2, 1, -1, -2, -2, -2],
  },
  {
    id: "electronic_punch",
    label: "Electronic Punch",
    bands: [4, 3, 2, 1, 0, 1, 2, 3, 2, 1, 0, -1],
  },
  {
    id: "acoustic_warm",
    label: "Acoustic Warm",
    bands: [1, 2, 2, 1, 0, 0, 1, 1, 0, -1, -1, -1],
  },
  {
    id: "late_night",
    label: "Late Night",
    bands: [3, 2, 1, 0, 0, 1, 2, 2, 1, 0, -1, -1],
  },
  {
    id: "rock",
    label: "Rock",
    bands: [3, 2, 1, 0, -1, 0, 1, 3, 3, 2, 1, 0],
  },
];

export function SettingsScreen({ category }: { category?: string }) {
  const stereodrome = useStereodrome();
  const mobileSettings = useMobileSettings();
  const view = useViewStack();
  const queryClient = useQueryClient();
  const busyActionRef = useRef<string | null>(null);
  const textEditSavingRef = useRef(false);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [textEdit, setTextEdit] = useState<TextEditConfig | null>(null);
  const [textEditError, setTextEditError] = useState<string | null>(null);
  const [textEditSaving, setTextEditSaving] = useState(false);
  const [textEditValue, setTextEditValue] = useState("");
  const selectedCategory = parseSettingsCategory(category);
  const syncStatus = useQuery({
    queryKey: librarySyncStatusQueryKey,
    queryFn: stereodromeCore.getLibrarySyncStatus,
    enabled: selectedCategory === "sync",
    refetchInterval: (query) => {
      const activeJob = query.state.data?.active_job;
      return activeJob !== null &&
        activeJob !== undefined &&
        activeJob.length > 0
        ? 2000
        : false;
    },
  });
  const syncSettings = useQuery({
    queryKey: syncSettingsQueryKey,
    queryFn: stereodromeCore.getSyncSettings,
    enabled: selectedCategory === "sync",
  });
  const scanStatus = useQuery({
    queryKey: scanStatusQueryKey,
    queryFn: stereodromeCore.getScanStatus,
    enabled:
      selectedCategory === "sync" &&
      stereodrome.status.connected &&
      !stereodrome.manualOfflineEnabled,
  });
  const cacheStats = useQuery({
    queryKey: ["audio-cache-stats"],
    queryFn: stereodromeCore.getAudioCacheStats,
    enabled: selectedCategory === "cache",
  });
  const lastfmStatus = useQuery({
    queryKey: lastfmStatusQueryKey,
    queryFn: stereodromeCore.getLastfmStatus,
    enabled: selectedCategory === "lastfm",
  });
  const lastfmQueue = useQuery({
    queryKey: lastfmQueueQueryKey,
    queryFn: stereodromeCore.getLastfmQueue,
    enabled: selectedCategory === "lastfm",
  });
  const audioSettings = useQuery({
    queryKey: ["audio-processing-settings"],
    queryFn: stereodromeCore.getAudioProcessingSettings,
    enabled:
      selectedCategory === "playback" || selectedCategory === "normalization",
  });
  const { protectedActionRows } = useProtectedSelectableAction(
    [
      selectedCategory ?? "root",
      stereodrome.manualOfflineEnabled,
      stereodrome.hasConfiguredServer,
      stereodrome.status.connected,
      lastfmStatus.data?.authenticated ?? false,
      syncStatus.data?.active_job ?? "idle",
      cacheStats.data?.file_count ?? 0,
      cacheStats.data?.total_size ?? 0,
    ].join(":")
  );

  function openTextEdit(config: TextEditConfig) {
    setTextEdit(config);
    setTextEditError(null);
    setTextEditValue(config.value);
  }

  useEffect(() => {
    if (selectedCategory !== "sync" || scanStatus.data?.scanning !== true) {
      return undefined;
    }

    const interval = setInterval(() => {
      void queryClient.invalidateQueries({ queryKey: scanStatusQueryKey });
    }, 2000);
    return () => {
      clearInterval(interval);
    };
  }, [queryClient, scanStatus.data?.scanning, selectedCategory]);

  const settings = audioSettings.data;
  const options = [
    ...(selectedCategory
      ? categoryOptions(selectedCategory)
      : settingsCategories.map((settingsCategory) => ({
          kind: "action" as const,
          label: settingsCategory.label,
          sublabel: settingsCategory.sublabel,
          onSelect: () => {
            view.push({
              name: "settings",
              title: settingsCategory.label,
              params: { category: settingsCategory.id },
            });
          },
        }))),
    ...messageOptions(),
  ];

  function categoryOptions(categoryId: SettingsCategory): SelectableOption[] {
    switch (categoryId) {
      case "server":
        return serverOptions();
      case "sync":
        return syncOptions();
      case "lastfm":
        return lastfmOptions();
      case "interface":
        return interfaceOptions();
      case "playback":
        return playbackOptions(settings, updateAudioSetting, openTextEdit);
      case "normalization":
        return normalizationOptions(settings, updateAudioSetting, openTextEdit);
      case "cache":
        return cacheOptions();
      default:
        return [];
    }
  }

  function serverOptions(): SelectableOption[] {
    return [
      ...protectedActionRows({
        id: "toggle-offline-mode",
        label: "Offline Mode",
        sublabel: stereodrome.manualOfflineEnabled
          ? "Downloaded songs only"
          : "Use server when available",
        confirmLabel: stereodrome.manualOfflineEnabled
          ? "Confirm Online"
          : "Confirm Offline",
        confirmSublabel: stereodrome.manualOfflineEnabled
          ? "Use wheel select to use server again"
          : "Use wheel select to use downloads only",
        cancelLabel: "Cancel Change",
        cancelSublabel: "Keep current connectivity mode",
        onConfirm: async () => {
          await runBusy("offline-mode", async () => {
            const nextEnabled = !stereodrome.manualOfflineEnabled;
            await stereodrome.setManualOfflineEnabled(nextEnabled);
            setMessage(
              nextEnabled ? "Offline mode enabled" : "Offline mode disabled"
            );
          });
        },
      }),
      {
        kind: "editable",
        label: "Server",
        sublabel: stereodrome.status.server_url ?? "Not connected",
        onSelect: () => {
          openTextEdit({
            title: "Server URL",
            value: stereodrome.status.server_url ?? "",
            keyboardType: "url",
            onSubmit: async (value) => {
              if (stereodrome.manualOfflineEnabled) {
                throw new Error("Offline mode is enabled");
              }
              const url = value.trim();
              if (!url) {
                throw new Error("Server URL is required");
              }
              await stereodrome.updateServerSettings({ url });
            },
          });
        },
      },
      {
        kind: "editable",
        label: "Username",
        sublabel: stereodrome.status.username ?? "-",
        onSelect: () => {
          openTextEdit({
            title: "Username",
            value: stereodrome.status.username ?? "",
            onSubmit: async (value) => {
              if (stereodrome.manualOfflineEnabled) {
                throw new Error("Offline mode is enabled");
              }
              const username = value.trim();
              if (!username) {
                throw new Error("Username is required");
              }
              await stereodrome.updateServerSettings({ username });
            },
          });
        },
      },
      {
        kind: "info",
        label: "Server Version",
        sublabel: stereodrome.status.server_version ?? "-",
        onSelect: () => stereodrome.refreshStatus(),
      },
      ...(stereodrome.hasConfiguredServer
        ? protectedActionRows({
            id: "disconnect-server",
            label: "Disconnect",
            sublabel: "Sign out of this server",
            confirmLabel: "Confirm Disconnect",
            confirmSublabel: "Use wheel select to sign out",
            cancelLabel: "Cancel Disconnect",
            cancelSublabel: "Keep server connection",
            onConfirm: async () => {
              await runBusy("disconnect", async () => {
                await stereodromeCore.disconnectServer();
                await stereodrome.refreshStatus();
                setMessage("Disconnected");
              });
            },
          })
        : []),
    ];
  }

  function syncOptions(): SelectableOption[] {
    const syncActions =
      stereodrome.status.connected && !stereodrome.manualOfflineEnabled
        ? [
            ...protectedActionRows({
              id: "start-scan",
              label: "Start scan",
              sublabel:
                busyAction === "scan"
                  ? "Starting..."
                  : scanStatus.data?.scanning === true
                    ? formatScanStatus(scanStatus.data)
                    : "Invoke Subsonic scan",
              confirmLabel: "Confirm Scan",
              confirmSublabel: "Use wheel select to ask server to scan",
              cancelLabel: "Cancel Scan",
              cancelSublabel: "Do not start server scan",
              onConfirm: () => runStartScan(),
            }),
            {
              kind: "action" as const,
              label: "Incremental sync",
              sublabel:
                busyAction === "incremental"
                  ? "Syncing..."
                  : `Last: ${formatTimestamp(syncStatus.data?.incremental.last_success_at)}`,
              onSelect: () => runSync("incremental"),
            },
            ...protectedActionRows({
              id: "full-sync",
              label: "Full sync",
              sublabel:
                busyAction === "full"
                  ? "Syncing..."
                  : `Last: ${formatTimestamp(syncStatus.data?.full_reconcile.last_success_at)}`,
              confirmLabel: "Confirm Full Sync",
              confirmSublabel: "Use wheel select to reconcile library",
              cancelLabel: "Cancel Full Sync",
              cancelSublabel: "Do not start full sync",
              onConfirm: () => runSync("full"),
            }),
          ]
        : [
            {
              kind: "info" as const,
              label: stereodrome.manualOfflineEnabled
                ? "Offline Mode"
                : "Disconnected",
              sublabel: stereodrome.manualOfflineEnabled
                ? "Turn off offline mode to sync"
                : "Reconnect to sync library",
              onSelect: () => stereodrome.refreshStatus(),
            },
          ];

    return [
      {
        kind: "info",
        label: "Library Sync",
        sublabel:
          syncStatus.data?.active_job !== null &&
          syncStatus.data?.active_job !== undefined &&
          syncStatus.data.active_job.length > 0
            ? syncStatus.data.active_job === "incremental"
              ? "Running incremental sync"
              : "Running full sync"
            : "Idle",
        onSelect: () =>
          queryClient.invalidateQueries({
            queryKey: librarySyncStatusQueryKey,
          }),
      },
      ...syncScheduleOptions(
        syncSettings.data,
        updateSyncSettings,
        openTextEdit
      ),
      ...syncActions,
      ...(hasText(syncStatus.data?.incremental.last_error)
        ? [
            {
              kind: "info" as const,
              label: "Incremental Error",
              sublabel: syncStatus.data.incremental.last_error,
              onSelect: () =>
                queryClient.invalidateQueries({
                  queryKey: librarySyncStatusQueryKey,
                }),
            },
          ]
        : []),
      ...(hasText(syncStatus.data?.full_reconcile.last_error)
        ? [
            {
              kind: "info" as const,
              label: "Full Sync Error",
              sublabel: syncStatus.data.full_reconcile.last_error,
              onSelect: () =>
                queryClient.invalidateQueries({
                  queryKey: librarySyncStatusQueryKey,
                }),
            },
          ]
        : []),
    ];
  }

  function lastfmOptions(): SelectableOption[] {
    const status = lastfmStatus.data;
    const queue = lastfmQueue.data ?? [];
    if (!status) {
      return [
        {
          kind: "info",
          label: "Last.fm",
          sublabel: "Loading...",
          onSelect: () =>
            queryClient.invalidateQueries({ queryKey: lastfmStatusQueryKey }),
        },
      ];
    }

    return [
      {
        kind: "info",
        label: "Status",
        sublabel: !status.available
          ? "Not configured"
          : status.authenticated
            ? `Connected${hasText(status.username) ? ` as ${status.username}` : ""}`
            : status.pending_auth
              ? "Authorization pending"
              : "Disconnected",
        onSelect: refreshLastfm,
      },
      {
        kind: "info",
        label: "Queued Scrobbles",
        sublabel: `${status.queue_count.toLocaleString()} pending`,
        onSelect: refreshLastfm,
      },
      ...(hasText(status.last_error)
        ? [
            {
              kind: "info" as const,
              label: "Last.fm Error",
              sublabel: status.last_error,
              onSelect: refreshLastfm,
            },
          ]
        : []),
      ...lastfmQueueOptions(queue),
      ...lastfmActionOptions(status),
    ];
  }

  function lastfmQueueOptions(queue: LastfmQueueItem[]): SelectableOption[] {
    return queue.slice(0, 6).map((item) => ({
      kind: "info" as const,
      label: item.title,
      sublabel: `${item.artist}${hasText(item.album) ? ` — ${item.album}` : ""}`,
      onSelect: refreshLastfm,
    }));
  }

  function lastfmActionOptions(
    status: NonNullable<typeof lastfmStatus.data>
  ): SelectableOption[] {
    if (stereodrome.manualOfflineEnabled) {
      return [
        {
          kind: "info",
          label: "Offline Mode",
          sublabel: "Turn off offline mode for Last.fm network actions",
          onSelect: refreshLastfm,
        },
      ];
    }

    const lastfmActions: SelectableOption[] = [];
    if (status.available && !status.authenticated) {
      lastfmActions.push({
        kind: "action",
        label: "Connect",
        sublabel:
          busyAction === "lastfm-connect"
            ? "Opening..."
            : "Authorize in browser",
        onSelect: () => runBeginLastfmAuth(),
      });
    }
    if (status.pending_auth) {
      lastfmActions.push({
        kind: "action",
        label: "Complete Authorization",
        sublabel:
          busyAction === "lastfm-complete"
            ? "Completing..."
            : "Return after approving Last.fm",
        onSelect: () => runCompleteLastfmAuth(),
      });
    }
    if (status.authenticated) {
      lastfmActions.push(
        {
          kind: "action",
          label: "Retry Queue",
          sublabel:
            busyAction === "lastfm-retry"
              ? "Retrying..."
              : "Submit pending scrobbles",
          onSelect: () => runRetryLastfmQueue(),
        },
        ...protectedActionRows({
          id: "disconnect-lastfm",
          label: "Disconnect",
          sublabel: "Remove Last.fm session",
          confirmLabel: "Confirm Disconnect",
          confirmSublabel: "Use wheel select to remove Last.fm session",
          cancelLabel: "Cancel Disconnect",
          cancelSublabel: "Keep Last.fm connected",
          onConfirm: () => runDisconnectLastfm(),
        })
      );
    }
    return lastfmActions;
  }

  function interfaceOptions(): SelectableOption[] {
    return [
      {
        kind: "editable",
        label: "Button Layout",
        sublabel:
          mobileSettings.buttonHandedness === "right"
            ? "Right handed"
            : "Left handed",
        onSelect: () => {
          mobileSettings.toggleButtonHandedness();
        },
      },
    ];
  }

  function cacheOptions(): SelectableOption[] {
    return [
      {
        kind: "info",
        label: "Audio Cache",
        sublabel: `${formatBytes(cacheStats.data?.total_size ?? 0)} used, ${
          cacheStats.data?.file_count ?? 0
        } files`,
        onSelect: () =>
          queryClient.invalidateQueries({ queryKey: ["audio-cache-stats"] }),
      },
      {
        kind: "editable",
        label: "Maximum Cache Size",
        sublabel: formatBytes(cacheStats.data?.max_size ?? 0),
        onSelect: () => {
          openTextEdit({
            title: "Maximum Cache Size (GB)",
            value: formatInputNumber(
              (cacheStats.data?.max_size ?? 0) / 1024 ** 3
            ),
            keyboardType: "decimal-pad",
            onSubmit: async (value) => {
              const gb = parseNumberInput(value, "Maximum cache size");
              if (gb <= 0) {
                throw new Error("Maximum cache size must be greater than 0");
              }
              await stereodromeCore.setMaxCacheSize(Math.round(gb * 1024 ** 3));
              await queryClient.invalidateQueries({
                queryKey: ["audio-cache-stats"],
              });
              setMessage(`Cache limit set to ${formatCacheSizePreset(gb)}`);
            },
          });
        },
        onLongSelect: () => cycleCacheSize(-1),
      },
      ...protectedActionRows({
        id: "settings-clear-audio-cache",
        label: "Clear Audio Cache",
        sublabel:
          busyAction === "clear-cache" ? "Clearing..." : "Remove cached audio",
        confirmLabel: "Confirm Clear",
        confirmSublabel: "Use wheel select to remove cached audio",
        cancelLabel: "Cancel Clear",
        cancelSublabel: "Keep cached audio",
        onConfirm: async () => {
          await runBusy("clear-cache", async () => {
            await stereodromeCore.clearAudioCache();
            await queryClient.invalidateQueries({
              queryKey: ["audio-cache-stats"],
            });
            await stereodrome.refreshOfflineSongIds();
            setMessage("Cache cleared");
          });
        },
      }),
    ];
  }

  function messageOptions(): SelectableOption[] {
    return hasText(message)
      ? [
          {
            kind: "info" as const,
            label: "Last Action",
            sublabel: message,
            onSelect: () => {
              setMessage(null);
            },
          },
        ]
      : [];
  }

  async function updateAudioSetting(patch: Partial<AudioProcessingSettings>) {
    if (!audioSettings.data) {
      return;
    }
    const next = await stereodromeCore.setAudioProcessingSettings({
      ...audioSettings.data,
      ...patch,
    });
    queryClient.setQueryData(["audio-processing-settings"], next);
  }

  async function updateSyncSettings(patch: Partial<SyncSettings>) {
    const current =
      syncSettings.data ?? (await stereodromeCore.getSyncSettings());
    const next = await stereodromeCore.setSyncSettings({
      ...current,
      ...patch,
    });
    queryClient.setQueryData(syncSettingsQueryKey, next);
    await configureLibrarySyncBackgroundTask(next);
    await queryClient.invalidateQueries({
      queryKey: librarySyncStatusQueryKey,
    });
  }

  async function runBusy(label: string, action: () => Promise<void>) {
    if (hasText(busyActionRef.current)) {
      return;
    }

    busyActionRef.current = label;
    setBusyAction(label);
    setMessage(null);
    try {
      await action();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      busyActionRef.current = null;
      setBusyAction(null);
    }
  }

  async function runStartScan() {
    await runBusy("scan", async () => {
      const status = await stereodromeCore.startScan();
      queryClient.setQueryData<ScanStatus>(scanStatusQueryKey, status);
      setMessage(status.scanning ? "Scan started" : "Scan requested");
    });
  }

  async function runSync(mode: "incremental" | "full") {
    await runBusy(mode, async () => {
      queryClient.setQueryData<LibrarySyncStatus>(
        librarySyncStatusQueryKey,
        (status) => markSyncStatusRunning(status, mode)
      );
      if (mode === "incremental") {
        await stereodrome.syncIncremental();
        setMessage("Incremental sync started");
      } else {
        await stereodrome.sync();
        setMessage("Full sync started");
      }
      await queryClient.invalidateQueries({
        queryKey: librarySyncStatusQueryKey,
      });
    });
  }

  async function refreshLastfm() {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: lastfmStatusQueryKey }),
      queryClient.invalidateQueries({ queryKey: lastfmQueueQueryKey }),
    ]);
  }

  async function runBeginLastfmAuth() {
    await runBusy("lastfm-connect", async () => {
      const auth = await stereodromeCore.beginLastfmAuth();
      await Linking.openURL(auth.auth_url);
      await refreshLastfm();
      setMessage("Approve Last.fm, then choose Complete Authorization");
    });
  }

  async function runCompleteLastfmAuth() {
    await runBusy("lastfm-complete", async () => {
      await stereodromeCore.completeLastfmAuth();
      await refreshLastfm();
      setMessage("Last.fm connected");
    });
  }

  async function runRetryLastfmQueue() {
    await runBusy("lastfm-retry", async () => {
      const submitted = await stereodromeCore.retryLastfmQueue();
      await refreshLastfm();
      setMessage(`Submitted ${submitted.toLocaleString()} scrobbles`);
    });
  }

  async function runDisconnectLastfm() {
    await runBusy("lastfm-disconnect", async () => {
      await stereodromeCore.disconnectLastfm();
      await refreshLastfm();
      setMessage("Last.fm disconnected");
    });
  }

  async function cycleCacheSize(direction: 1 | -1) {
    const currentGb = (cacheStats.data?.max_size ?? 0) / 1024 ** 3;
    const nextGb = cycleNumber(cacheSizePresetsGb, currentGb, direction);
    await runBusy("cache-size", async () => {
      await stereodromeCore.setMaxCacheSize(Math.round(nextGb * 1024 ** 3));
      await queryClient.invalidateQueries({
        queryKey: ["audio-cache-stats"],
      });
      setMessage(`Cache limit set to ${formatCacheSizePreset(nextGb)}`);
    });
  }

  async function submitTextEdit() {
    if (!textEdit || textEditSavingRef.current) {
      return;
    }

    textEditSavingRef.current = true;
    setTextEditSaving(true);
    setMessage(null);
    setTextEditError(null);
    try {
      await textEdit.onSubmit(textEditValue);
      setTextEdit(null);
    } catch (e) {
      setTextEditError(e instanceof Error ? e.message : String(e));
    } finally {
      textEditSavingRef.current = false;
      setTextEditSaving(false);
    }
  }

  return (
    <View style={styles.container}>
      <SelectableList
        disabled={textEdit !== null || busyAction !== null}
        empty="Settings unavailable"
        options={options}
        preserveSelectionOnChange
        resetSelectionKey={selectedCategory ?? "root"}
      />
      <Modal
        animationType="fade"
        onRequestClose={() => {
          setTextEdit(null);
        }}
        transparent
        visible={textEdit !== null}
      >
        <View style={styles.modalOverlay}>
          <View style={styles.modalCard}>
            <Text style={styles.modalTitle}>{textEdit?.title}</Text>
            <TextInput
              autoFocus
              keyboardType={textEdit?.keyboardType ?? "default"}
              onChangeText={setTextEditValue}
              onSubmitEditing={() => {
                void submitTextEdit();
              }}
              selectTextOnFocus
              style={styles.modalInput}
              value={textEditValue}
            />
            {hasText(textEditError) ? (
              <Text numberOfLines={2} style={styles.modalError}>
                {textEditError}
              </Text>
            ) : null}
            <View style={styles.modalActions}>
              <Pressable
                disabled={textEditSaving}
                onPress={() => {
                  setTextEdit(null);
                }}
                style={styles.modalButton}
              >
                <Text style={styles.modalButtonText}>Cancel</Text>
              </Pressable>
              <Pressable
                disabled={textEditSaving}
                onPress={() => {
                  void submitTextEdit();
                }}
                style={[styles.modalButton, styles.modalPrimaryButton]}
              >
                <Text style={styles.modalPrimaryButtonText}>
                  {textEditSaving ? "Saving..." : "Save"}
                </Text>
              </Pressable>
            </View>
          </View>
        </View>
      </Modal>
    </View>
  );
}

function syncScheduleOptions(
  settings: SyncSettings | undefined,
  updateSyncSettings: (patch: Partial<SyncSettings>) => Promise<void>,
  openTextEdit: (config: TextEditConfig) => void
): SelectableOption[] {
  if (!settings) {
    return [
      {
        kind: "info",
        label: "Scheduled Sync",
        sublabel: "Loading...",
        onSelect: () => {
          // This informational loading row intentionally has no action.
        },
      },
    ];
  }

  return [
    {
      kind: "editable",
      label: "Periodic incremental sync",
      sublabel: onOff(settings.incremental_enabled),
      onSelect: () =>
        updateSyncSettings({
          incremental_enabled: !settings.incremental_enabled,
        }),
    },
    ...(settings.incremental_enabled
      ? [
          {
            kind: "editable" as const,
            label: "Partial sync interval",
            sublabel: formatMinutes(settings.incremental_interval_minutes),
            onSelect: () => {
              openTextEdit({
                title: "Partial Sync Interval (minutes)",
                value: formatInputNumber(settings.incremental_interval_minutes),
                keyboardType: "number-pad",
                onSubmit: async (value) => {
                  const minutes = parseNumberInput(
                    value,
                    "Partial sync interval"
                  );
                  await updateSyncSettings({
                    incremental_interval_minutes: Math.round(
                      clamp(minutes, 5, 720)
                    ),
                  });
                },
              });
            },
            onLongSelect: () =>
              updateSyncSettings({
                incremental_interval_minutes: cycleNumber(
                  incrementalSyncIntervals,
                  settings.incremental_interval_minutes,
                  1
                ),
              }),
          },
        ]
      : []),
    {
      kind: "editable",
      label: "Periodic full reconcile",
      sublabel: onOff(settings.full_reconcile_enabled),
      onSelect: () =>
        updateSyncSettings({
          full_reconcile_enabled: !settings.full_reconcile_enabled,
        }),
    },
    ...(settings.full_reconcile_enabled
      ? [
          {
            kind: "editable" as const,
            label: "Full reconcile interval",
            sublabel: formatHours(settings.full_reconcile_interval_hours),
            onSelect: () => {
              openTextEdit({
                title: "Full Reconcile Interval (hours)",
                value: formatInputNumber(
                  settings.full_reconcile_interval_hours
                ),
                keyboardType: "number-pad",
                onSubmit: async (value) => {
                  const hours = parseNumberInput(
                    value,
                    "Full reconcile interval"
                  );
                  await updateSyncSettings({
                    full_reconcile_interval_hours: Math.round(
                      clamp(hours, 1, 168)
                    ),
                  });
                },
              });
            },
            onLongSelect: () =>
              updateSyncSettings({
                full_reconcile_interval_hours: cycleNumber(
                  fullReconcileIntervals,
                  settings.full_reconcile_interval_hours,
                  1
                ),
              }),
          },
        ]
      : []),
  ];
}

function playbackOptions(
  settings: AudioProcessingSettings | undefined,
  updateAudioSetting: (
    patch: Partial<AudioProcessingSettings>
  ) => Promise<void>,
  openTextEdit: (config: TextEditConfig) => void
): SelectableOption[] {
  if (!settings) {
    return [
      {
        kind: "info",
        label: "Playback",
        sublabel: "Loading...",
        onSelect: () => {
          // This informational loading row intentionally has no action.
        },
      },
    ];
  }

  const activeEqPreset = getEqPreset(settings.equalizer_bands_db);
  return [
    {
      kind: "editable",
      label: "Gapless Playback",
      sublabel: onOff(settings.gapless_enabled),
      onSelect: () =>
        updateAudioSetting({ gapless_enabled: !settings.gapless_enabled }),
    },
    {
      kind: "editable",
      label: "Files to Prefetch",
      sublabel: `${settings.prefetch_count} upcoming`,
      onSelect: () => {
        openTextEdit({
          title: "Files to Prefetch",
          value: String(settings.prefetch_count),
          keyboardType: "number-pad",
          onSubmit: async (value) => {
            const count = parseNumberInput(value, "Files to prefetch");
            await updateAudioSetting({
              prefetch_count: Math.round(clamp(count, 1, 10)),
            });
          },
        });
      },
      onLongSelect: () =>
        updateAudioSetting({
          prefetch_count: cycleNumber(
            prefetchCountPresets,
            settings.prefetch_count,
            1
          ),
        }),
    },
    {
      kind: "editable",
      label: "Crossfade",
      sublabel: onOff(settings.crossfade_enabled),
      onSelect: () =>
        updateAudioSetting({ crossfade_enabled: !settings.crossfade_enabled }),
    },
    ...(settings.crossfade_enabled
      ? [
          {
            kind: "editable" as const,
            label: "Crossfade Duration",
            sublabel: `${settings.crossfade_duration_ms / 1000}s`,
            onSelect: () => {
              openTextEdit({
                title: "Crossfade Duration (seconds)",
                value: formatInputNumber(settings.crossfade_duration_ms / 1000),
                keyboardType: "decimal-pad",
                onSubmit: async (value) => {
                  const seconds = parseNumberInput(value, "Crossfade duration");
                  await updateAudioSetting({
                    crossfade_duration_ms: Math.round(
                      clamp(seconds, 0.5, 15) * 1000
                    ),
                  });
                },
              });
            },
            onLongSelect: () =>
              updateAudioSetting({
                crossfade_duration_ms: cycleNumber(
                  crossfadePresets,
                  settings.crossfade_duration_ms,
                  -1
                ),
              }),
          },
        ]
      : []),
    {
      kind: "editable",
      label: "Binaural Crossfeed",
      sublabel: onOff(settings.binaural_enabled),
      onSelect: () =>
        updateAudioSetting({ binaural_enabled: !settings.binaural_enabled }),
    },
    ...(settings.binaural_enabled
      ? [
          {
            kind: "editable" as const,
            label: "Binaural Preset",
            sublabel: labelForPreset(settings.binaural_preset),
            onSelect: () =>
              updateAudioSetting({
                binaural_preset: cycleString(
                  ["light", "medium", "strong"] as const,
                  settings.binaural_preset,
                  1
                ),
              }),
            onLongSelect: () =>
              updateAudioSetting({
                binaural_preset: cycleString(
                  ["light", "medium", "strong"] as const,
                  settings.binaural_preset,
                  -1
                ),
              }),
          },
        ]
      : []),
    {
      kind: "editable",
      label: "Equalizer",
      sublabel: onOff(settings.equalizer_enabled),
      onSelect: () =>
        updateAudioSetting({ equalizer_enabled: !settings.equalizer_enabled }),
    },
    {
      kind: "editable",
      label: "EQ Preset",
      sublabel:
        eqPresets.find((preset) => preset.id === activeEqPreset)?.label ??
        "Custom",
      onSelect: () =>
        updateAudioSetting({
          equalizer_enabled: true,
          equalizer_bands_db:
            eqPresets[
              nextPresetIndex(
                eqPresets.map((preset) => preset.id),
                activeEqPreset,
                1
              )
            ]?.bands ?? settings.equalizer_bands_db,
        }),
      onLongSelect: () =>
        updateAudioSetting({
          equalizer_enabled: true,
          equalizer_bands_db:
            eqPresets[
              nextPresetIndex(
                eqPresets.map((preset) => preset.id),
                activeEqPreset,
                -1
              )
            ]?.bands ?? settings.equalizer_bands_db,
        }),
    },
    ...sanitizeEqBands(settings.equalizer_bands_db).map((band, index) => ({
      kind: "editable" as const,
      label: `EQ ${getEqLabel(index)}`,
      sublabel: formatDb(band),
      onSelect: () => {
        openTextEdit({
          title: `EQ ${getEqLabel(index)} (dB)`,
          value: formatInputNumber(band),
          keyboardType: "numbers-and-punctuation",
          onSubmit: async (value) => {
            const db = parseNumberInput(value, `EQ ${getEqLabel(index)}`);
            await updateAudioSetting({
              equalizer_enabled: true,
              equalizer_bands_db: setBand(
                settings.equalizer_bands_db,
                index,
                clamp(db, eqMinDb, eqMaxDb)
              ),
            });
          },
        });
      },
      onLongSelect: () =>
        updateAudioSetting({
          equalizer_enabled: true,
          equalizer_bands_db: updateBand(
            settings.equalizer_bands_db,
            index,
            -eqStepDb
          ),
        }),
    })),
  ];
}

function normalizationOptions(
  settings: AudioProcessingSettings | undefined,
  updateAudioSetting: (
    patch: Partial<AudioProcessingSettings>
  ) => Promise<void>,
  openTextEdit: (config: TextEditConfig) => void
): SelectableOption[] {
  if (!settings) {
    return [
      {
        kind: "info",
        label: "Volume Normalization",
        sublabel: "Loading...",
        onSelect: () => {
          // This informational loading row intentionally has no action.
        },
      },
    ];
  }

  return [
    {
      kind: "editable",
      label: "Volume Normalization",
      sublabel: onOff(settings.normalization_enabled),
      onSelect: () =>
        updateAudioSetting({
          normalization_enabled: !settings.normalization_enabled,
        }),
    },
    ...(settings.normalization_enabled
      ? [
          {
            kind: "editable" as const,
            label: "Normalization Mode",
            sublabel: labelForPreset(settings.normalization_mode),
            onSelect: () =>
              updateAudioSetting({
                normalization_mode:
                  settings.normalization_mode === "track" ? "album" : "track",
              }),
          },
          {
            kind: "editable" as const,
            label: "Target Level",
            sublabel: `${settings.target_lufs} LUFS`,
            onSelect: () => {
              openTextEdit({
                title: "Target Level (LUFS)",
                value: formatInputNumber(settings.target_lufs),
                keyboardType: "numbers-and-punctuation",
                onSubmit: async (value) => {
                  const lufs = parseNumberInput(value, "Target level");
                  await updateAudioSetting({
                    target_lufs: clamp(lufs, -24, -8),
                  });
                },
              });
            },
            onLongSelect: () =>
              updateAudioSetting({
                target_lufs: cycleNumber(lufsPresets, settings.target_lufs, -1),
              }),
          },
          {
            kind: "editable" as const,
            label: "Preamp",
            sublabel: formatDb(settings.preamp_db),
            onSelect: () => {
              openTextEdit({
                title: "Preamp (dB)",
                value: formatInputNumber(settings.preamp_db),
                keyboardType: "numbers-and-punctuation",
                onSubmit: async (value) => {
                  const db = parseNumberInput(value, "Preamp");
                  await updateAudioSetting({
                    preamp_db: clamp(db, -12, 12),
                  });
                },
              });
            },
            onLongSelect: () =>
              updateAudioSetting({
                preamp_db: clamp(settings.preamp_db - 0.5, -6, 6),
              }),
          },
          {
            kind: "editable" as const,
            label: "Prevent Clipping",
            sublabel: onOff(settings.prevent_clipping),
            onSelect: () =>
              updateAudioSetting({
                prevent_clipping: !settings.prevent_clipping,
              }),
          },
          {
            kind: "editable" as const,
            label: "Dynamics Processing",
            sublabel: onOff(settings.dynamics_enabled),
            onSelect: () =>
              updateAudioSetting({
                dynamics_enabled: !settings.dynamics_enabled,
              }),
          },
          ...(settings.dynamics_enabled
            ? [
                {
                  kind: "editable" as const,
                  label: "Dynamics Amount",
                  sublabel: labelForPreset(settings.dynamics_preset),
                  onSelect: () =>
                    updateAudioSetting({
                      dynamics_preset: cycleString(
                        ["light", "medium", "heavy"] as const,
                        settings.dynamics_preset,
                        1
                      ),
                    }),
                  onLongSelect: () =>
                    updateAudioSetting({
                      dynamics_preset: cycleString(
                        ["light", "medium", "heavy"] as const,
                        settings.dynamics_preset,
                        -1
                      ),
                    }),
                },
              ]
            : []),
        ]
      : []),
  ];
}

function formatBytes(bytes: number) {
  if (bytes <= 0) {
    return "0 B";
  }
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KB`;
  }
  if (bytes < 1024 * 1024 * 1024) {
    return `${Math.round(bytes / (1024 * 1024))} MB`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function formatTimestamp(value: string | null | undefined) {
  if (!hasText(value)) {
    return "Never";
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return "Invalid date";
  }
  return parsed.toLocaleString();
}

function formatMinutes(minutes: number) {
  return minutes >= 60 && minutes % 60 === 0
    ? `${minutes / 60}h`
    : `${minutes}m`;
}

function formatHours(hours: number) {
  return `${hours}h`;
}

function parseSettingsCategory(value: string | undefined) {
  return (
    settingsCategories.find((settingsCategory) => settingsCategory.id === value)
      ?.id ?? null
  );
}

function markSyncStatusRunning(
  status: LibrarySyncStatus | undefined,
  mode: "incremental" | "full"
) {
  if (!status) {
    return status;
  }

  const key = syncJobKey(mode);
  return {
    ...status,
    active_job: key,
    [key]: {
      ...status[key],
      running: true,
      last_attempt_at: new Date().toISOString(),
      last_error: null,
    },
  };
}

function syncJobKey(mode: "incremental" | "full") {
  return mode === "full" ? "full_reconcile" : mode;
}

function formatScanStatus(status: ScanStatus) {
  if (status.scanning) {
    return status.count !== null && status.count !== 0
      ? `Scanning (${status.count.toLocaleString()} items)`
      : "Scanning...";
  }
  return status.count !== null && status.count !== 0
    ? `Idle (${status.count.toLocaleString()} items)`
    : "Idle";
}

function hasText(value: string | null | undefined): value is string {
  return value !== null && value !== undefined && value.length > 0;
}

function onOff(value: boolean) {
  return value ? "On" : "Off";
}

function labelForPreset(value: string) {
  return value
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function cycleNumber(values: number[], current: number, direction: 1 | -1) {
  const index = values.findIndex((value) => Math.abs(value - current) < 0.01);
  const safeIndex = index === -1 ? 0 : index;
  return (
    values[(safeIndex + direction + values.length) % values.length] ?? current
  );
}

function cycleString<T extends string>(
  values: readonly T[],
  current: string,
  direction: 1 | -1
): T {
  const index = values.findIndex((value) => value === current);
  const safeIndex = index === -1 ? 0 : index;
  const nextValue =
    values[(safeIndex + direction + values.length) % values.length];
  if (nextValue === undefined) {
    throw new Error("Cannot cycle an empty list of values");
  }
  return nextValue;
}

function nextPresetIndex(
  values: EqPresetId[],
  current: EqPresetId | null,
  direction: 1 | -1
) {
  const index = current ? values.indexOf(current) : -1;
  const safeIndex = index === -1 ? 0 : index;
  return (safeIndex + direction + values.length) % values.length;
}

function sanitizeEqBands(bands: number[] | undefined) {
  const output = Array<number>(12).fill(0);
  if (!bands) {
    return output;
  }

  for (let index = 0; index < Math.min(12, bands.length); index += 1) {
    output[index] = clamp(bands[index] ?? 0, eqMinDb, eqMaxDb);
  }
  return output;
}

function getEqLabel(index: number) {
  return eqLabels[index] ?? `Band ${index + 1}`;
}

function getEqPreset(bands: number[] | undefined): EqPresetId | null {
  const normalized = sanitizeEqBands(bands);
  return (
    eqPresets.find((preset) =>
      preset.bands.every(
        (value, index) => Math.abs(value - (normalized[index] ?? 0)) <= 0.05
      )
    )?.id ?? null
  );
}

function updateBand(bands: number[], index: number, delta: number) {
  return sanitizeEqBands(bands).map((band, bandIndex) =>
    bandIndex === index ? clamp(band + delta, eqMinDb, eqMaxDb) : band
  );
}

function setBand(bands: number[], index: number, value: number) {
  return sanitizeEqBands(bands).map((band, bandIndex) =>
    bandIndex === index ? value : band
  );
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

function parseNumberInput(value: string, label: string) {
  const parsed = Number(value.trim().replace(",", "."));
  if (!Number.isFinite(parsed)) {
    throw new Error(`${label} must be a number`);
  }
  return parsed;
}

function formatInputNumber(value: number) {
  return Number.isInteger(value) ? `${value}` : `${Number(value.toFixed(2))}`;
}

function formatDb(value: number) {
  return `${value > 0 ? "+" : ""}${value.toFixed(1)} dB`;
}

function formatCacheSizePreset(value: number) {
  return value < 1 ? `${value * 1000}MB` : `${value}GB`;
}
