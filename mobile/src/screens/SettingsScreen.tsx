import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  type KeyboardTypeOptions,
  Modal,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";

import {
  SelectableList,
  type SelectableOption,
} from "@/components/SelectableList";
import { colors } from "@/components/theme";
import { useMobileSettings } from "@/context/MobileSettingsContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { useViewStack } from "@/context/ViewContext";
import { stereodromeCore } from "@/services/stereodromeCore";
import type { AudioProcessingSettings } from "@/types/music";

const lufsPresets = [-18, -16, -14, -12, -10];
const crossfadePresets = [1000, 3000, 5000, 8000, 12000];
const cacheSizePresetsGb = [0.5, 1, 2, 5, 10, 20, 50];
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
  | "interface"
  | "playback"
  | "normalization"
  | "cache";

const settingsCategories: Array<{
  id: SettingsCategory;
  label: string;
  sublabel: string;
}> = [
  { id: "server", label: "Server", sublabel: "Connection and account" },
  { id: "sync", label: "Library Sync", sublabel: "Sync status and actions" },
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
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [textEdit, setTextEdit] = useState<TextEditConfig | null>(null);
  const [textEditError, setTextEditError] = useState<string | null>(null);
  const [textEditSaving, setTextEditSaving] = useState(false);
  const [textEditValue, setTextEditValue] = useState("");
  const syncStatus = useQuery({
    queryKey: ["library-sync-status"],
    queryFn: stereodromeCore.getLibrarySyncStatus,
  });
  const cacheStats = useQuery({
    queryKey: ["audio-cache-stats"],
    queryFn: stereodromeCore.getAudioCacheStats,
  });
  const audioSettings = useQuery({
    queryKey: ["audio-processing-settings"],
    queryFn: stereodromeCore.getAudioProcessingSettings,
  });

  function openTextEdit(config: TextEditConfig) {
    setTextEdit(config);
    setTextEditError(null);
    setTextEditValue(config.value);
  }

  const selectedCategory = parseSettingsCategory(category);
  const settings = audioSettings.data;
  const options = [
    ...(selectedCategory
      ? categoryOptions(selectedCategory)
      : settingsCategories.map((settingsCategory) => ({
          kind: "action" as const,
          label: settingsCategory.label,
          sublabel: settingsCategory.sublabel,
          onSelect: () =>
            view.push({
              name: "settings",
              title: settingsCategory.label,
              params: { category: settingsCategory.id },
            }),
        }))),
    ...messageOptions(),
  ];

  function categoryOptions(categoryId: SettingsCategory): SelectableOption[] {
    switch (categoryId) {
      case "server":
        return serverOptions();
      case "sync":
        return syncOptions();
      case "interface":
        return interfaceOptions();
      case "playback":
        return playbackOptions(settings, updateAudioSetting, openTextEdit);
      case "normalization":
        return normalizationOptions(settings, updateAudioSetting, openTextEdit);
      case "cache":
        return cacheOptions();
    }
  }

  function serverOptions(): SelectableOption[] {
    return [
      {
        kind: "editable",
        label: "Server",
        sublabel: stereodrome.status.server_url ?? "Not connected",
        onSelect: () =>
          openTextEdit({
            title: "Server URL",
            value: stereodrome.status.server_url ?? "",
            keyboardType: "url",
            onSubmit: async (value) => {
              const url = value.trim();
              if (!url) {
                throw new Error("Server URL is required");
              }
              await stereodrome.updateServerSettings({ url });
            },
          }),
      },
      {
        kind: "editable",
        label: "Username",
        sublabel: stereodrome.status.username ?? "-",
        onSelect: () =>
          openTextEdit({
            title: "Username",
            value: stereodrome.status.username ?? "",
            onSubmit: async (value) => {
              const username = value.trim();
              if (!username) {
                throw new Error("Username is required");
              }
              await stereodrome.updateServerSettings({ username });
            },
          }),
      },
      {
        kind: "info",
        label: "Server Version",
        sublabel: stereodrome.status.server_version ?? "-",
        onSelect: () => stereodrome.refreshStatus(),
      },
      ...(stereodrome.status.connected
        ? [
            {
              kind: "action" as const,
              label: "Disconnect",
              sublabel: "Sign out of this server",
              onSelect: async () => {
                await runBusy("disconnect", async () => {
                  await stereodromeCore.disconnectServer();
                  await stereodrome.refreshStatus();
                  setMessage("Disconnected");
                });
              },
            },
          ]
        : []),
    ];
  }

  function syncOptions(): SelectableOption[] {
    return [
      {
        kind: "info",
        label: "Library Sync",
        sublabel: syncStatus.data?.active_job
          ? syncStatus.data.active_job === "incremental"
            ? "Running incremental sync"
            : syncStatus.data.active_job === "full"
              ? "Running full sync"
              : "Running full reconcile"
          : "Idle",
        onSelect: () =>
          queryClient.invalidateQueries({ queryKey: ["library-sync-status"] }),
      },
      {
        kind: "action",
        label: "Full Sync",
        sublabel:
          busyAction === "full"
            ? "Syncing..."
            : `Last: ${formatTimestamp(syncStatus.data?.full.last_success_at)}`,
        onSelect: () => runSync("full"),
      },
      {
        kind: "action",
        label: "Incremental Sync",
        sublabel:
          busyAction === "incremental"
            ? "Syncing..."
            : `Last: ${formatTimestamp(syncStatus.data?.incremental.last_success_at)}`,
        onSelect: () => runSync("incremental"),
      },
      {
        kind: "action",
        label: "Full Reconcile",
        sublabel:
          busyAction === "reconcile"
            ? "Reconciling..."
            : `Last: ${formatTimestamp(syncStatus.data?.full_reconcile.last_success_at)}`,
        onSelect: () => runSync("reconcile"),
      },
      ...(syncStatus.data?.incremental.last_error
        ? [
            {
              kind: "info" as const,
              label: "Incremental Error",
              sublabel: syncStatus.data.incremental.last_error,
              onSelect: () =>
                queryClient.invalidateQueries({
                  queryKey: ["library-sync-status"],
                }),
            },
          ]
        : []),
      ...(syncStatus.data?.full.last_error
        ? [
            {
              kind: "info" as const,
              label: "Full Sync Error",
              sublabel: syncStatus.data.full.last_error,
              onSelect: () =>
                queryClient.invalidateQueries({
                  queryKey: ["library-sync-status"],
                }),
            },
          ]
        : []),
      ...(syncStatus.data?.full_reconcile.last_error
        ? [
            {
              kind: "info" as const,
              label: "Reconcile Error",
              sublabel: syncStatus.data.full_reconcile.last_error,
              onSelect: () =>
                queryClient.invalidateQueries({
                  queryKey: ["library-sync-status"],
                }),
            },
          ]
        : []),
    ];
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
        onSelect: () => mobileSettings.toggleButtonHandedness(),
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
        onSelect: () =>
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
          }),
        onLongSelect: () => cycleCacheSize(-1),
      },
      {
        kind: "action",
        label: "Clear Audio Cache",
        sublabel:
          busyAction === "clear-cache" ? "Clearing..." : "Remove cached audio",
        onSelect: async () => {
          await runBusy("clear-cache", async () => {
            await stereodromeCore.clearAudioCache();
            await queryClient.invalidateQueries({
              queryKey: ["audio-cache-stats"],
            });
            setMessage("Cache cleared");
          });
        },
      },
    ];
  }

  function messageOptions(): SelectableOption[] {
    return message
      ? [
          {
            kind: "info" as const,
            label: "Last Action",
            sublabel: message,
            onSelect: () => setMessage(null),
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

  async function runBusy(label: string, action: () => Promise<void>) {
    if (busyAction) {
      return;
    }

    setBusyAction(label);
    setMessage(null);
    try {
      await action();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyAction(null);
    }
  }

  async function runSync(mode: "full" | "incremental" | "reconcile") {
    await runBusy(mode, async () => {
      if (mode === "full") {
        await stereodrome.sync();
        setMessage("Full sync complete");
      } else if (mode === "incremental") {
        await stereodrome.syncIncremental();
        setMessage("Incremental sync complete");
      } else {
        await stereodrome.reconcile();
        setMessage("Full reconcile complete");
      }
      await queryClient.invalidateQueries();
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
    if (!textEdit || textEditSaving) {
      return;
    }

    setTextEditSaving(true);
    setMessage(null);
    setTextEditError(null);
    try {
      await textEdit.onSubmit(textEditValue);
      setTextEdit(null);
    } catch (e) {
      setTextEditError(e instanceof Error ? e.message : String(e));
    } finally {
      setTextEditSaving(false);
    }
  }

  return (
    <View style={styles.container}>
      <SelectableList
        disabled={textEdit !== null}
        empty="Settings unavailable"
        options={options}
        preserveSelectionOnChange
      />
      <Modal
        animationType="fade"
        onRequestClose={() => setTextEdit(null)}
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
              onSubmitEditing={submitTextEdit}
              selectTextOnFocus
              style={styles.modalInput}
              value={textEditValue}
            />
            {textEditError ? (
              <Text numberOfLines={2} style={styles.modalError}>
                {textEditError}
              </Text>
            ) : null}
            <View style={styles.modalActions}>
              <Pressable
                disabled={textEditSaving}
                onPress={() => setTextEdit(null)}
                style={styles.modalButton}
              >
                <Text style={styles.modalButtonText}>Cancel</Text>
              </Pressable>
              <Pressable
                disabled={textEditSaving}
                onPress={submitTextEdit}
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
        onSelect: () => {},
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
            onSelect: () =>
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
              }),
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
            ].bands,
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
            ].bands,
        }),
    },
    ...sanitizeEqBands(settings.equalizer_bands_db).map((band, index) => ({
      kind: "editable" as const,
      label: `EQ ${eqLabels[index]}`,
      sublabel: formatDb(band),
      onSelect: () =>
        openTextEdit({
          title: `EQ ${eqLabels[index]} (dB)`,
          value: formatInputNumber(band),
          keyboardType: "numbers-and-punctuation",
          onSubmit: async (value) => {
            const db = parseNumberInput(value, `EQ ${eqLabels[index]}`);
            await updateAudioSetting({
              equalizer_enabled: true,
              equalizer_bands_db: setBand(
                settings.equalizer_bands_db,
                index,
                clamp(db, eqMinDb, eqMaxDb)
              ),
            });
          },
        }),
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
        onSelect: () => {},
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
            onSelect: () =>
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
              }),
            onLongSelect: () =>
              updateAudioSetting({
                target_lufs: cycleNumber(lufsPresets, settings.target_lufs, -1),
              }),
          },
          {
            kind: "editable" as const,
            label: "Preamp",
            sublabel: formatDb(settings.preamp_db),
            onSelect: () =>
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
              }),
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
  if (!value) {
    return "Never";
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return "Invalid date";
  }
  return parsed.toLocaleString();
}

function parseSettingsCategory(value: string | undefined) {
  return settingsCategories.some((category) => category.id === value)
    ? (value as SettingsCategory)
    : null;
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
  return values[(safeIndex + direction + values.length) % values.length];
}

function cycleString<T extends string>(
  values: readonly T[],
  current: string,
  direction: 1 | -1
): T {
  const index = values.findIndex((value) => value === current);
  const safeIndex = index === -1 ? 0 : index;
  return values[(safeIndex + direction + values.length) % values.length];
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

function getEqPreset(bands: number[] | undefined): EqPresetId | null {
  const normalized = sanitizeEqBands(bands);
  return (
    eqPresets.find((preset) =>
      preset.bands.every(
        (value, index) => Math.abs(value - normalized[index]) <= 0.05
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

const styles = StyleSheet.create({
  container: {
    flex: 1,
  },
  modalActions: {
    flexDirection: "row",
    gap: 8,
    justifyContent: "flex-end",
  },
  modalButton: {
    borderColor: "#b9b9b2",
    borderRadius: 4,
    borderWidth: 1,
    paddingHorizontal: 10,
    paddingVertical: 7,
  },
  modalButtonText: {
    color: colors.text,
    fontSize: 12,
    fontWeight: "800",
  },
  modalCard: {
    backgroundColor: "#f7f7ef",
    borderColor: "#b9b9b2",
    borderRadius: 8,
    borderWidth: 1,
    padding: 12,
    width: "82%",
  },
  modalInput: {
    backgroundColor: "#fff",
    borderColor: "#c9c9c1",
    borderRadius: 4,
    borderWidth: 1,
    color: colors.text,
    fontSize: 15,
    fontWeight: "700",
    height: 36,
    marginBottom: 10,
    paddingHorizontal: 8,
  },
  modalError: {
    color: "#b3261e",
    fontSize: 11,
    fontWeight: "700",
    marginBottom: 8,
  },
  modalOverlay: {
    alignItems: "center",
    backgroundColor: "rgba(0, 0, 0, 0.38)",
    flex: 1,
    justifyContent: "center",
    padding: 14,
  },
  modalPrimaryButton: {
    backgroundColor: colors.selected,
    borderColor: colors.selected,
  },
  modalPrimaryButtonText: {
    color: colors.selectedText,
    fontSize: 12,
    fontWeight: "800",
  },
  modalTitle: {
    color: colors.text,
    fontSize: 15,
    fontWeight: "800",
    marginBottom: 8,
  },
});
