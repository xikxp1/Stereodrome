import { File, Paths } from "expo-file-system";
import * as Sharing from "expo-sharing";

import NativeStereodromeCore from "../../modules/stereodrome-core/src";

export type ResourceDiagnosticsStatus = {
  active: boolean;
  started_at: string | null;
  stopped_at: string | null;
  stopped_reason: "manual" | "sample_limit" | null;
  sample_count: number;
  sampling_interval_seconds: number;
  max_samples: number;
};

const unavailable =
  "Resource diagnostics are not available in this development build";

export async function startResourceDiagnostics() {
  const start = NativeStereodromeCore.startResourceDiagnostics;
  if (!start) {
    throw new Error(unavailable);
  }
  return parseStatus(await start());
}

export async function stopResourceDiagnostics() {
  const stop = NativeStereodromeCore.stopResourceDiagnostics;
  if (!stop) {
    throw new Error(unavailable);
  }
  return parseStatus(await stop());
}

export async function getResourceDiagnosticsStatus() {
  const getStatus = NativeStereodromeCore.getResourceDiagnosticsStatus;
  if (!getStatus) {
    throw new Error(unavailable);
  }
  return parseStatus(await getStatus());
}

export async function clearResourceDiagnostics() {
  const clear = NativeStereodromeCore.clearResourceDiagnostics;
  if (!clear) {
    throw new Error(unavailable);
  }
  if (!(await clear())) {
    throw new Error("Unable to clear the diagnostics session");
  }
}

export async function shareResourceDiagnostics() {
  const exportReport = NativeStereodromeCore.exportResourceDiagnostics;
  if (!exportReport) {
    throw new Error(unavailable);
  }
  if (!(await Sharing.isAvailableAsync())) {
    throw new Error("File sharing is not available on this device");
  }

  const timestamp = new Date().toISOString().replaceAll(":", "-");
  const file = new File(
    Paths.cache,
    `stereodrome-resource-diagnostics-${timestamp}.json`
  );
  if (file.exists) {
    file.delete();
  }
  try {
    if (!(await exportReport(nativeFilePath(file.uri)))) {
      throw new Error("Unable to export the diagnostics report");
    }
    await Sharing.shareAsync(file.uri, {
      dialogTitle: "Share Stereodrome Diagnostics",
      mimeType: "application/json",
      UTI: "public.json",
    });
  } finally {
    if (file.exists) {
      file.delete();
    }
  }
}

function parseStatus(raw: string): ResourceDiagnosticsStatus {
  const value: unknown = JSON.parse(raw);
  if (
    !isRecord(value) ||
    typeof value["active"] !== "boolean" ||
    !isNullableString(value["started_at"]) ||
    !isNullableString(value["stopped_at"]) ||
    (value["stopped_reason"] !== null &&
      value["stopped_reason"] !== "manual" &&
      value["stopped_reason"] !== "sample_limit") ||
    !isNonNegativeInteger(value["sample_count"]) ||
    !isNonNegativeInteger(value["sampling_interval_seconds"]) ||
    !isNonNegativeInteger(value["max_samples"])
  ) {
    throw new Error("Native diagnostics returned an invalid status");
  }
  return {
    active: value["active"],
    started_at: value["started_at"],
    stopped_at: value["stopped_at"],
    stopped_reason: value["stopped_reason"],
    sample_count: value["sample_count"],
    sampling_interval_seconds: value["sampling_interval_seconds"],
    max_samples: value["max_samples"],
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0;
}

function nativeFilePath(uri: string) {
  if (!uri.startsWith("file://")) {
    throw new Error("Diagnostics report is not available as a local file");
  }
  return decodeURIComponent(uri.slice("file://".length));
}
