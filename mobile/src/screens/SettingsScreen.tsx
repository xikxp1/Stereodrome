import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import {
  SelectableList,
  type SelectableOption,
} from "@/components/SelectableList";
import { useMobileSettings } from "@/context/MobileSettingsContext";
import { useStereodrome } from "@/context/StereodromeContext";
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

export function SettingsScreen() {
  const stereodrome = useStereodrome();
  const mobileSettings = useMobileSettings();
  const queryClient = useQueryClient();
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
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

  const settings = audioSettings.data;
  const options: SelectableOption[] = [
    {
      label: "Server",
      sublabel: stereodrome.status.server_url ?? "Not connected",
      onSelect: () => stereodrome.refreshStatus(),
    },
    {
      label: "Username",
      sublabel: stereodrome.status.username ?? "-",
      onSelect: () => stereodrome.refreshStatus(),
    },
    {
      label: "Server Version",
      sublabel: stereodrome.status.server_version ?? "-",
      onSelect: () => stereodrome.refreshStatus(),
    },
    ...(stereodrome.status.connected
      ? [
          {
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
    {
      label: "Library Sync",
      sublabel: syncStatus.data?.active_job
        ? syncStatus.data.active_job === "incremental"
          ? "Running incremental sync"
          : "Running full reconcile"
        : "Idle",
      onSelect: () =>
        queryClient.invalidateQueries({ queryKey: ["library-sync-status"] }),
    },
    {
      label: "Incremental Sync",
      sublabel:
        busyAction === "incremental"
          ? "Syncing..."
          : `Last: ${formatTimestamp(syncStatus.data?.incremental.last_success_at)}`,
      onSelect: () => runSync("incremental"),
    },
    {
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
            label: "Incremental Error",
            sublabel: syncStatus.data.incremental.last_error,
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
            label: "Reconcile Error",
            sublabel: syncStatus.data.full_reconcile.last_error,
            onSelect: () =>
              queryClient.invalidateQueries({
                queryKey: ["library-sync-status"],
              }),
          },
        ]
      : []),
    {
      label: "Button Layout",
      sublabel:
        mobileSettings.buttonHandedness === "right"
          ? "Right handed"
          : "Left handed",
      onSelect: () => mobileSettings.toggleButtonHandedness(),
    },
    ...playbackOptions(settings, updateAudioSetting),
    ...normalizationOptions(settings, updateAudioSetting),
    {
      label: "Audio Cache",
      sublabel: `${formatBytes(cacheStats.data?.total_size ?? 0)} used, ${
        cacheStats.data?.file_count ?? 0
      } files`,
      onSelect: () =>
        queryClient.invalidateQueries({ queryKey: ["audio-cache-stats"] }),
    },
    {
      label: "Maximum Cache Size",
      sublabel: formatBytes(cacheStats.data?.max_size ?? 0),
      onSelect: () => cycleCacheSize(1),
      onLongSelect: () => cycleCacheSize(-1),
    },
    {
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
    ...(message
      ? [
          {
            label: "Last Action",
            sublabel: message,
            onSelect: () => setMessage(null),
          },
        ]
      : []),
  ];

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

  async function runSync(mode: "incremental" | "reconcile") {
    await runBusy(mode, async () => {
      if (mode === "incremental") {
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

  return (
    <SelectableList
      empty="Settings unavailable"
      options={options}
      preserveSelectionOnChange
    />
  );
}

function playbackOptions(
  settings: AudioProcessingSettings | undefined,
  updateAudioSetting: (patch: Partial<AudioProcessingSettings>) => Promise<void>
): SelectableOption[] {
  if (!settings) {
    return [
      {
        label: "Playback",
        sublabel: "Loading...",
        onSelect: () => {},
      },
    ];
  }

  const activeEqPreset = getEqPreset(settings.equalizer_bands_db);
  return [
    {
      label: "Gapless Playback",
      sublabel: onOff(settings.gapless_enabled),
      onSelect: () =>
        updateAudioSetting({ gapless_enabled: !settings.gapless_enabled }),
    },
    {
      label: "Crossfade",
      sublabel: onOff(settings.crossfade_enabled),
      onSelect: () =>
        updateAudioSetting({ crossfade_enabled: !settings.crossfade_enabled }),
    },
    ...(settings.crossfade_enabled
      ? [
          {
            label: "Crossfade Duration",
            sublabel: `${settings.crossfade_duration_ms / 1000}s`,
            onSelect: () =>
              updateAudioSetting({
                crossfade_duration_ms: cycleNumber(
                  crossfadePresets,
                  settings.crossfade_duration_ms,
                  1
                ),
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
      label: "Binaural Crossfeed",
      sublabel: onOff(settings.binaural_enabled),
      onSelect: () =>
        updateAudioSetting({ binaural_enabled: !settings.binaural_enabled }),
    },
    ...(settings.binaural_enabled
      ? [
          {
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
      label: "Equalizer",
      sublabel: onOff(settings.equalizer_enabled),
      onSelect: () =>
        updateAudioSetting({ equalizer_enabled: !settings.equalizer_enabled }),
    },
    {
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
      label: `EQ ${eqLabels[index]}`,
      sublabel: formatDb(band),
      onSelect: () =>
        updateAudioSetting({
          equalizer_enabled: true,
          equalizer_bands_db: updateBand(
            settings.equalizer_bands_db,
            index,
            eqStepDb
          ),
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
  updateAudioSetting: (patch: Partial<AudioProcessingSettings>) => Promise<void>
): SelectableOption[] {
  if (!settings) {
    return [
      {
        label: "Volume Normalization",
        sublabel: "Loading...",
        onSelect: () => {},
      },
    ];
  }

  return [
    {
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
            label: "Normalization Mode",
            sublabel: labelForPreset(settings.normalization_mode),
            onSelect: () =>
              updateAudioSetting({
                normalization_mode:
                  settings.normalization_mode === "track" ? "album" : "track",
              }),
          },
          {
            label: "Target Level",
            sublabel: `${settings.target_lufs} LUFS`,
            onSelect: () =>
              updateAudioSetting({
                target_lufs: cycleNumber(lufsPresets, settings.target_lufs, 1),
              }),
            onLongSelect: () =>
              updateAudioSetting({
                target_lufs: cycleNumber(lufsPresets, settings.target_lufs, -1),
              }),
          },
          {
            label: "Preamp",
            sublabel: formatDb(settings.preamp_db),
            onSelect: () =>
              updateAudioSetting({
                preamp_db: clamp(settings.preamp_db + 0.5, -6, 6),
              }),
            onLongSelect: () =>
              updateAudioSetting({
                preamp_db: clamp(settings.preamp_db - 0.5, -6, 6),
              }),
          },
          {
            label: "Prevent Clipping",
            sublabel: onOff(settings.prevent_clipping),
            onSelect: () =>
              updateAudioSetting({
                prevent_clipping: !settings.prevent_clipping,
              }),
          },
          {
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

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

function formatDb(value: number) {
  return `${value > 0 ? "+" : ""}${value.toFixed(1)} dB`;
}

function formatCacheSizePreset(value: number) {
  return value < 1 ? `${value * 1000}MB` : `${value}GB`;
}
