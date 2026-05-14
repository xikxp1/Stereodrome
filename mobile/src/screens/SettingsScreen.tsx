import { useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  ScrollView,
  StyleSheet,
  Switch,
  Text,
  View,
} from "react-native";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { colors } from "@/components/theme";
import { useMobileSettings } from "@/context/MobileSettingsContext";
import { useStereodrome } from "@/context/StereodromeContext";
import { stereodromeCore } from "@/services/stereodromeCore";
import type { AudioProcessingSettings } from "@/types/music";

const targetLufsPresets = [-18, -16, -14, -12, -10];
const crossfadePresets = [2000, 5000, 8000, 12000];
const eqLabels = [
  "31",
  "62",
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
];

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
  const audioSettings = useQuery({
    queryKey: ["audio-processing-settings"],
    queryFn: stereodromeCore.getAudioProcessingSettings,
  });

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
    <ScrollView
      contentContainerStyle={styles.container}
      showsVerticalScrollIndicator={false}
    >
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
      <Text style={styles.heading}>Audio Processing</Text>
      <SettingSwitch
        label="Normalization"
        value={audioSettings.data?.normalization_enabled ?? false}
        onValueChange={(value) =>
          updateAudioSetting({ normalization_enabled: value })
        }
      />
      {audioSettings.data?.normalization_enabled ? (
        <>
          <SegmentedControl
            label="Mode"
            options={[
              { label: "Track", value: "track" },
              { label: "Album", value: "album" },
            ]}
            value={audioSettings.data.normalization_mode}
            onChange={(value) =>
              updateAudioSetting({ normalization_mode: value })
            }
          />
          <PresetRow
            label={`Target ${audioSettings.data.target_lufs} LUFS`}
            options={targetLufsPresets.map((value) => ({
              label: String(value),
              value,
            }))}
            value={audioSettings.data.target_lufs}
            onChange={(value) => updateAudioSetting({ target_lufs: value })}
          />
          <StepperRow
            label="Preamp"
            suffix=" dB"
            value={audioSettings.data.preamp_db}
            step={1}
            min={-12}
            max={12}
            onChange={(value) => updateAudioSetting({ preamp_db: value })}
          />
          <SettingSwitch
            label="Prevent clipping"
            value={audioSettings.data.prevent_clipping}
            onValueChange={(value) =>
              updateAudioSetting({ prevent_clipping: value })
            }
          />
        </>
      ) : null}
      <SettingSwitch
        label="Equalizer"
        value={audioSettings.data?.equalizer_enabled ?? false}
        onValueChange={(value) =>
          updateAudioSetting({ equalizer_enabled: value })
        }
      />
      {audioSettings.data?.equalizer_enabled ? (
        <>
          <PresetRow
            label="EQ Preset"
            options={[
              { label: "Flat", value: "flat" },
              { label: "Bass", value: "bass" },
              { label: "Vocal", value: "vocal" },
              { label: "Bright", value: "bright" },
            ]}
            value={eqPreset(audioSettings.data.equalizer_bands_db)}
            onChange={(value) =>
              updateAudioSetting({
                equalizer_bands_db: eqPresetBands(value),
              })
            }
          />
          <EqualizerBands
            bands={audioSettings.data.equalizer_bands_db}
            onChange={(bands) =>
              updateAudioSetting({ equalizer_bands_db: bands })
            }
          />
        </>
      ) : null}
      <SettingSwitch
        label="Dynamics"
        value={audioSettings.data?.dynamics_enabled ?? false}
        onValueChange={(value) =>
          updateAudioSetting({ dynamics_enabled: value })
        }
      />
      {audioSettings.data?.dynamics_enabled ? (
        <SegmentedControl
          label="Dynamics preset"
          options={[
            { label: "Light", value: "light" },
            { label: "Medium", value: "medium" },
            { label: "Heavy", value: "heavy" },
          ]}
          value={audioSettings.data.dynamics_preset}
          onChange={(value) => updateAudioSetting({ dynamics_preset: value })}
        />
      ) : null}
      <SettingSwitch
        label="Binaural"
        value={audioSettings.data?.binaural_enabled ?? false}
        onValueChange={(value) =>
          updateAudioSetting({ binaural_enabled: value })
        }
      />
      {audioSettings.data?.binaural_enabled ? (
        <SegmentedControl
          label="Binaural preset"
          options={[
            { label: "Light", value: "light" },
            { label: "Medium", value: "medium" },
            { label: "Strong", value: "strong" },
          ]}
          value={audioSettings.data.binaural_preset}
          onChange={(value) => updateAudioSetting({ binaural_preset: value })}
        />
      ) : null}
      <SettingSwitch
        label="Gapless"
        value={audioSettings.data?.gapless_enabled ?? false}
        onValueChange={(value) =>
          updateAudioSetting({ gapless_enabled: value })
        }
      />
      <SettingSwitch
        label="Crossfade"
        value={audioSettings.data?.crossfade_enabled ?? false}
        onValueChange={(value) =>
          updateAudioSetting({ crossfade_enabled: value })
        }
      />
      {audioSettings.data?.crossfade_enabled ? (
        <PresetRow
          label={`${audioSettings.data.crossfade_duration_ms / 1000}s duration`}
          options={crossfadePresets.map((value) => ({
            label: `${value / 1000}s`,
            value,
          }))}
          value={audioSettings.data.crossfade_duration_ms}
          onChange={(value) =>
            updateAudioSetting({ crossfade_duration_ms: value })
          }
        />
      ) : null}
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
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  container: {
    padding: 12,
    paddingBottom: 24,
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
  settingRow: {
    alignItems: "center",
    flexDirection: "row",
    justifyContent: "space-between",
    minHeight: 36,
  },
  controlLabel: {
    color: colors.muted,
    fontSize: 11,
    fontWeight: "800",
    marginBottom: 4,
    marginTop: 6,
    textTransform: "uppercase",
  },
  presetRow: {
    flexDirection: "row",
    gap: 6,
    marginBottom: 8,
  },
  presetButton: {
    alignItems: "center",
    borderColor: colors.screenBorder,
    borderRadius: 4,
    borderWidth: 2,
    flex: 1,
    minHeight: 28,
    justifyContent: "center",
    paddingHorizontal: 4,
  },
  presetButtonSelected: {
    backgroundColor: colors.selected,
  },
  presetButtonText: {
    color: colors.text,
    fontSize: 11,
    fontWeight: "800",
  },
  presetButtonTextSelected: {
    color: colors.selectedText,
  },
  stepperRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: 8,
    minHeight: 34,
  },
  stepperButton: {
    alignItems: "center",
    borderColor: colors.screenBorder,
    borderRadius: 4,
    borderWidth: 2,
    height: 28,
    justifyContent: "center",
    width: 34,
  },
  stepperValue: {
    color: colors.text,
    flex: 1,
    fontSize: 12,
    fontWeight: "800",
    textAlign: "center",
  },
  eqGrid: {
    gap: 6,
    marginBottom: 8,
  },
  eqRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: 6,
  },
  eqLabel: {
    color: colors.muted,
    fontSize: 10,
    fontWeight: "800",
    width: 28,
  },
  settingLabel: {
    color: colors.text,
    fontSize: 13,
    fontWeight: "800",
  },
  buttonText: {
    color: "#fff",
    fontWeight: "800",
  },
});

function SegmentedControl<T extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: { label: string; value: T }[];
  value: T;
  onChange(value: T): void;
}) {
  return (
    <>
      <Text style={styles.controlLabel}>{label}</Text>
      <View style={styles.segmentedControl}>
        {options.map((option) => {
          const selected = option.value === value;
          return (
            <Pressable
              key={option.value}
              onPress={() => onChange(option.value)}
              style={[styles.segment, selected && styles.segmentSelected]}
            >
              <Text
                style={[
                  styles.segmentText,
                  selected && styles.segmentSelectedText,
                ]}
              >
                {option.label}
              </Text>
            </Pressable>
          );
        })}
      </View>
    </>
  );
}

function PresetRow<T extends string | number>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: { label: string; value: T }[];
  value: T;
  onChange(value: T): void;
}) {
  return (
    <>
      <Text style={styles.controlLabel}>{label}</Text>
      <View style={styles.presetRow}>
        {options.map((option) => {
          const selected = option.value === value;
          return (
            <Pressable
              key={String(option.value)}
              onPress={() => onChange(option.value)}
              style={[
                styles.presetButton,
                selected && styles.presetButtonSelected,
              ]}
            >
              <Text
                style={[
                  styles.presetButtonText,
                  selected && styles.presetButtonTextSelected,
                ]}
              >
                {option.label}
              </Text>
            </Pressable>
          );
        })}
      </View>
    </>
  );
}

function StepperRow({
  label,
  suffix = "",
  value,
  step,
  min,
  max,
  onChange,
}: {
  label: string;
  suffix?: string;
  value: number;
  step: number;
  min: number;
  max: number;
  onChange(value: number): void;
}) {
  return (
    <>
      <Text style={styles.controlLabel}>{label}</Text>
      <View style={styles.stepperRow}>
        <Pressable
          onPress={() => onChange(clamp(value - step, min, max))}
          style={styles.stepperButton}
        >
          <Text style={styles.presetButtonText}>-</Text>
        </Pressable>
        <Text style={styles.stepperValue}>
          {formatNumber(value)}
          {suffix}
        </Text>
        <Pressable
          onPress={() => onChange(clamp(value + step, min, max))}
          style={styles.stepperButton}
        >
          <Text style={styles.presetButtonText}>+</Text>
        </Pressable>
      </View>
    </>
  );
}

function EqualizerBands({
  bands,
  onChange,
}: {
  bands: number[];
  onChange(bands: number[]): void;
}) {
  const normalizedBands = [...bands, ...Array(12).fill(0)].slice(0, 12);
  return (
    <>
      <Text style={styles.controlLabel}>12-band EQ</Text>
      <View style={styles.eqGrid}>
        {normalizedBands.map((band, index) => (
          <View key={eqLabels[index]} style={styles.eqRow}>
            <Text style={styles.eqLabel}>{eqLabels[index]}</Text>
            <Pressable
              onPress={() => onChange(updateBand(normalizedBands, index, -1))}
              style={styles.stepperButton}
            >
              <Text style={styles.presetButtonText}>-</Text>
            </Pressable>
            <Text style={styles.stepperValue}>{formatNumber(band)} dB</Text>
            <Pressable
              onPress={() => onChange(updateBand(normalizedBands, index, 1))}
              style={styles.stepperButton}
            >
              <Text style={styles.presetButtonText}>+</Text>
            </Pressable>
          </View>
        ))}
      </View>
    </>
  );
}

function SettingSwitch({
  label,
  value,
  onValueChange,
}: {
  label: string;
  value: boolean;
  onValueChange(value: boolean): void;
}) {
  return (
    <View style={styles.settingRow}>
      <Text style={styles.settingLabel}>{label}</Text>
      <Switch value={value} onValueChange={onValueChange} />
    </View>
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KB`;
  }
  if (bytes < 1024 * 1024 * 1024) {
    return `${Math.round(bytes / (1024 * 1024))} MB`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

function formatNumber(value: number) {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

function updateBand(bands: number[], index: number, delta: number) {
  return bands.map((band, bandIndex) =>
    bandIndex === index ? clamp(band + delta, -12, 12) : band
  );
}

function eqPreset(bands: number[]) {
  const normalized = [...bands, ...Array(12).fill(0)].slice(0, 12);
  const presets = ["flat", "bass", "vocal", "bright"] as const;
  return (
    presets.find(
      (preset) =>
        JSON.stringify(eqPresetBands(preset)) === JSON.stringify(normalized)
    ) ?? "custom"
  );
}

function eqPresetBands(preset: string) {
  switch (preset) {
    case "bass":
      return [5, 4, 3, 2, 1, 0, 0, -1, -1, -1, -1, -1];
    case "vocal":
      return [-2, -1, 0, 2, 3, 4, 3, 2, 0, -1, -2, -2];
    case "bright":
      return [-2, -2, -1, 0, 0, 1, 2, 3, 4, 5, 5, 4];
    case "flat":
    default:
      return Array(12).fill(0);
  }
}
