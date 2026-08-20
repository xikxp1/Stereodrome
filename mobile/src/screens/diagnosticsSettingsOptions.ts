import type { SelectableOption } from "@/components/SelectableList";
import type { ProtectedSelectableAction } from "@/components/protectedSelectableAction";
import {
  clearResourceDiagnostics,
  shareResourceDiagnostics,
  startResourceDiagnostics,
  stopResourceDiagnostics,
  type ResourceDiagnosticsStatus,
} from "@/services/resourceDiagnostics";

type DiagnosticsSettingsOptionsParams = {
  status: ResourceDiagnosticsStatus | undefined;
  loadingError: string | null;
  busyAction: string | null;
  protectedActionRows(action: ProtectedSelectableAction): SelectableOption[];
  runBusy(label: string, action: () => Promise<void>): Promise<void>;
  setMessage(message: string): void;
  onRefresh(): void;
  onStatusChange(status: ResourceDiagnosticsStatus): void;
  onCleared(): void;
};

export function diagnosticsSettingsOptions({
  status,
  loadingError,
  busyAction,
  protectedActionRows,
  runBusy,
  setMessage,
  onRefresh,
  onStatusChange,
  onCleared,
}: DiagnosticsSettingsOptionsParams): SelectableOption[] {
  if (!status) {
    return [
      {
        kind: "info",
        label: "Diagnostics",
        sublabel: loadingError ?? "Loading...",
        onSelect: onRefresh,
      },
    ];
  }

  const rows: SelectableOption[] = [
    {
      kind: "info",
      label: status.active ? "Recording" : "Session",
      sublabel:
        status.sample_count === 0
          ? "No report recorded"
          : `${status.sample_count.toLocaleString()} samples since ${formatTimestamp(status.started_at)}`,
      onSelect: onRefresh,
    },
  ];

  if (status.active) {
    rows.push({
      kind: "action",
      label: "Stop Recording",
      sublabel:
        busyAction === "stop-diagnostics"
          ? "Stopping..."
          : "Finish and keep this session",
      onSelect: () =>
        runBusy("stop-diagnostics", async () => {
          const next = await stopResourceDiagnostics();
          onStatusChange(next);
          setMessage(`Recorded ${next.sample_count.toLocaleString()} samples`);
        }),
    });
  } else {
    rows.push(
      ...protectedActionRows({
        id: "start-resource-diagnostics",
        label: "Start Recording",
        sublabel:
          status.sample_count > 0
            ? "Replace the existing report"
            : "Sample resource use every 10 seconds",
        confirmLabel: "Confirm Start",
        confirmSublabel:
          status.sample_count > 0
            ? "Use wheel select to replace the saved report"
            : "Use wheel select to begin diagnostics",
        cancelLabel: "Cancel Start",
        cancelSublabel: "Keep the current diagnostics state",
        onConfirm: () =>
          runBusy("start-diagnostics", async () => {
            const next = await startResourceDiagnostics();
            onStatusChange(next);
            setMessage("Resource diagnostics started");
          }),
      })
    );
    if (status.sample_count > 0) {
      rows.push(
        {
          kind: "action",
          label: "Share Report",
          sublabel:
            busyAction === "share-diagnostics"
              ? "Preparing..."
              : "Export privacy-safe JSON",
          onSelect: () =>
            runBusy("share-diagnostics", async () => {
              await shareResourceDiagnostics();
              setMessage("Diagnostics report shared");
            }),
        },
        ...protectedActionRows({
          id: "clear-resource-diagnostics",
          label: "Clear Report",
          sublabel: "Delete the saved diagnostics session",
          confirmLabel: "Confirm Clear",
          confirmSublabel: "Use wheel select to delete this report",
          cancelLabel: "Cancel Clear",
          cancelSublabel: "Keep the saved report",
          onConfirm: () =>
            runBusy("clear-diagnostics", async () => {
              await clearResourceDiagnostics();
              onCleared();
              setMessage("Diagnostics report cleared");
            }),
        })
      );
    }
  }

  rows.push({
    kind: "info",
    label: "Report Contents",
    sublabel: "CPU, memory, battery, thermal, network, and storage",
    onSelect: () => {
      setMessage(
        "Also includes lifecycle and playback state; no credentials, server URLs, or media metadata"
      );
    },
  });
  return rows;
}

function formatTimestamp(value: string | null) {
  if (value === null || value.length === 0) {
    return "Never";
  }
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime())
    ? "Invalid date"
    : parsed.toLocaleString();
}
