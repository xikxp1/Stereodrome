import * as DocumentPicker from "expo-document-picker";
import { File, Paths } from "expo-file-system";
import * as Sharing from "expo-sharing";

import { coreClient } from "@/core/client";
import type { BackupSummary } from "@/types/music";

export async function sharePortableBackup(): Promise<BackupSummary> {
  if (!(await Sharing.isAvailableAsync())) {
    throw new Error("File sharing is not available on this device");
  }
  const date = new Date().toISOString().slice(0, 10);
  const file = new File(Paths.cache, `stereodrome-backup-${date}.json`);
  if (file.exists) {
    file.delete();
  }
  const summary = await coreClient.dispatchTyped({
    type: "export-portable-backup",
    path: nativeFilePath(file.uri),
  });
  try {
    await Sharing.shareAsync(file.uri, {
      dialogTitle: "Export Stereodrome Backup",
      mimeType: "application/json",
      UTI: "public.json",
    });
    return summary;
  } finally {
    if (file.exists) {
      file.delete();
    }
  }
}

export async function pickAndImportPortableBackup(): Promise<BackupSummary | null> {
  const selection = await DocumentPicker.getDocumentAsync({
    type: "application/json",
    copyToCacheDirectory: true,
    multiple: false,
  });
  if (selection.canceled) {
    return null;
  }
  const asset = selection.assets[0];
  if (!asset) {
    return null;
  }
  const file = new File(asset.uri);
  try {
    return await coreClient.dispatchTyped({
      type: "import-portable-backup",
      path: nativeFilePath(asset.uri),
    });
  } finally {
    if (file.exists) {
      file.delete();
    }
  }
}

function nativeFilePath(uri: string) {
  if (!uri.startsWith("file://")) {
    throw new Error("Selected backup is not available as a local file");
  }
  return decodeURIComponent(uri.slice("file://".length));
}
