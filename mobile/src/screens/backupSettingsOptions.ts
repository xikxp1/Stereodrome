import type { SelectableOption } from "@/components/SelectableList";
import type { ProtectedSelectableAction } from "@/components/protectedSelectableAction";
import {
  pickAndImportPortableBackup,
  sharePortableBackup,
} from "@/services/portableBackup";
import type { BackupSummary } from "@/types/music";

type BackupSettingsOptions = {
  busyAction: string | null;
  protectedActionRows(action: ProtectedSelectableAction): SelectableOption[];
  runBusy(label: string, action: () => Promise<void>): Promise<void>;
  setMessage(message: string): void;
  onImported(summary: BackupSummary): Promise<void>;
};

export function backupSettingsOptions({
  busyAction,
  protectedActionRows,
  runBusy,
  setMessage,
  onImported,
}: BackupSettingsOptions): SelectableOption[] {
  return [
    {
      kind: "action",
      label: "Export Backup",
      sublabel:
        busyAction === "export-backup"
          ? "Preparing backup..."
          : "Share library metadata and preferences",
      onSelect: async () => {
        await runBusy("export-backup", async () => {
          const summary = await sharePortableBackup();
          setMessage(
            `Prepared ${summary.songs.toLocaleString()} songs for sharing`
          );
        });
      },
    },
    ...protectedActionRows({
      id: "settings-import-backup",
      label: "Import Backup",
      sublabel:
        busyAction === "import-backup"
          ? "Importing backup..."
          : "Replace local library data",
      confirmLabel: "Confirm Import",
      confirmSublabel: "Replace library, playlists, queue, and preferences",
      cancelLabel: "Cancel Import",
      cancelSublabel: "Keep existing local data",
      onConfirm: async () => {
        await runBusy("import-backup", async () => {
          const summary = await pickAndImportPortableBackup();
          if (summary) {
            try {
              await onImported(summary);
            } catch {
              setMessage(
                `Imported ${summary.songs.toLocaleString()} songs; restart to refresh all views`
              );
            }
          }
        });
      },
    }),
    {
      kind: "info",
      label: "Not Included",
      sublabel: "Accounts, passwords, tokens, and cached media",
      onSelect: () => {
        setMessage("Backups contain metadata and portable preferences only");
      },
    },
  ];
}
