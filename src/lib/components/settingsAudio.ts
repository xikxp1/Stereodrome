import type { BinauralPreset, DynamicsPreset } from "$lib/types";

export const dynamicsPresets: DynamicsPreset[] = ["light", "medium", "heavy"];
export const dynamicsDescriptions: Record<DynamicsPreset, string> = {
  light: "Gentle compression. Preserves dynamics.",
  medium: "Balanced compression for mixed playlists.",
  heavy: "Strong compression. Maximum consistency.",
};

export const binauralPresets: BinauralPreset[] = [
  "default",
  "cmoy",
  "jmeier",
  "aggressive",
];
export const binauralDescriptions: Record<BinauralPreset, string> = {
  default: "Moderate crossfeed (700Hz / 4.5dB).",
  cmoy: "Subtle crossffed that matches famous Chu Moy analog circuit.",
  jmeier: "Jan Meier profile with the most subtle crossfeed effect.",
  aggressive: "Max-strength crossfeed for obvious A/B testing.",
};

export const EQ_MIN_DB = -12;
export const EQ_MAX_DB = 12;
const EQ_BANDS = 12;
export const EQ_BAND_LABELS = [
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

export type EqPresetId =
  | "flat"
  | "bass_boost"
  | "treble_sparkle"
  | "vocal_clarity"
  | "electronic_punch"
  | "acoustic_warm"
  | "late_night"
  | "rock";

interface EqPreset {
  id: EqPresetId;
  label: string;
  description: string;
  bands: number[];
}

export const eqPresets: EqPreset[] = [
  {
    id: "flat",
    label: "Flat",
    description: "Neutral response with no tonal shaping.",
    bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  },
  {
    id: "bass_boost",
    label: "Bass Boost",
    description: "Adds low-end weight while keeping mids clear.",
    bands: [5, 4, 3, 2, 1, 0, -1, -2, -2, -1, 0, 0],
  },
  {
    id: "treble_sparkle",
    label: "Treble Sparkle",
    description: "Brightens cymbals, air, and detail.",
    bands: [-2, -2, -1, 0, 0, 0, 1, 2, 3, 4, 4, 3],
  },
  {
    id: "vocal_clarity",
    label: "Vocal Clarity",
    description: "Pushes vocal presence and reduces boom.",
    bands: [-2, -1, 0, 2, 3, 3, 2, 1, -1, -2, -2, -2],
  },
  {
    id: "electronic_punch",
    label: "Electronic Punch",
    description: "Tight lows with crisp top-end for EDM.",
    bands: [4, 3, 2, 1, 0, 1, 2, 3, 2, 1, 0, -1],
  },
  {
    id: "acoustic_warm",
    label: "Acoustic Warm",
    description: "Natural warmth for strings and live recordings.",
    bands: [1, 2, 2, 1, 0, 0, 1, 1, 0, -1, -1, -1],
  },
  {
    id: "late_night",
    label: "Late Night",
    description: "Low-volume friendly smile curve.",
    bands: [3, 2, 1, 0, 0, 1, 2, 2, 1, 0, -1, -1],
  },
  {
    id: "rock",
    label: "Rock",
    description: "Adds punch and edge for guitars and drums.",
    bands: [3, 2, 1, 0, -1, 0, 1, 3, 3, 2, 1, 0],
  },
];

export function sanitizeEqBands(bands: number[] | undefined): number[] {
  const output = new Array<number>(EQ_BANDS).fill(0);
  if (!bands) return output;

  for (let i = 0; i < Math.min(EQ_BANDS, bands.length); i += 1) {
    output[i] = Math.min(EQ_MAX_DB, Math.max(EQ_MIN_DB, bands[i] ?? 0));
  }
  return output;
}

export function getEqPreset(bands: number[]): EqPresetId | null {
  const tolerance = 0.05;
  const normalized = sanitizeEqBands(bands);

  for (const preset of eqPresets) {
    const matches = preset.bands.every(
      (value, index) => Math.abs((normalized[index] ?? 0) - value) <= tolerance
    );
    if (matches) return preset.id;
  }
  return null;
}

export const lufsPresets = [-18, -16, -14, -12, -10];
export const sizePresets = [0.5, 1, 2, 5, 10, 20, 50];
