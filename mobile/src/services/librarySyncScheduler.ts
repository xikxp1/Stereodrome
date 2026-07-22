import * as BackgroundTask from "expo-background-task";
import * as TaskManager from "expo-task-manager";

import { stereodromeCore } from "@/services/stereodromeCore";
import type { SyncSettings } from "@/types/music";

const TASK_NAME = "stereodrome-library-sync";
const MIN_OS_BACKGROUND_INTERVAL_MINUTES = 15;

TaskManager.defineTask(TASK_NAME, async () => {
  try {
    await stereodromeCore.initialize();
    await stereodromeCore.runDueLibrarySync();
    return BackgroundTask.BackgroundTaskResult.Success;
  } catch {
    return BackgroundTask.BackgroundTaskResult.Failed;
  }
});

export async function syncLibraryBackgroundRegistration(): Promise<void> {
  const settings = await stereodromeCore.getSyncSettings();
  await configureLibrarySyncBackgroundTask(settings);
}

export async function configureLibrarySyncBackgroundTask(
  settings: SyncSettings | null
): Promise<void> {
  const minimumInterval = settings ? backgroundMinimumInterval(settings) : null;

  try {
    const registered = await TaskManager.isTaskRegisteredAsync(TASK_NAME);
    if (minimumInterval === null) {
      if (registered) {
        await BackgroundTask.unregisterTaskAsync(TASK_NAME);
      }
      return;
    }

    if (registered) {
      const options = await TaskManager.getTaskOptionsAsync<{
        minimumInterval?: number;
      }>(TASK_NAME);
      if (options.minimumInterval === minimumInterval) {
        return;
      }
      await BackgroundTask.unregisterTaskAsync(TASK_NAME);
    }

    await BackgroundTask.registerTaskAsync(TASK_NAME, {
      minimumInterval,
    });
  } catch {
    // Background tasks are unavailable in some development/runtime contexts.
  }
}

export async function triggerBackgroundSyncForTesting(): Promise<boolean> {
  return BackgroundTask.triggerTaskWorkerForTestingAsync();
}

function backgroundMinimumInterval(settings: SyncSettings): number | null {
  const intervals = [
    settings.incremental_enabled ? settings.incremental_interval_minutes : null,
    settings.full_reconcile_enabled
      ? settings.full_reconcile_interval_hours * 60
      : null,
  ].filter((interval): interval is number => typeof interval === "number");

  if (intervals.length === 0) {
    return null;
  }

  return Math.max(MIN_OS_BACKGROUND_INTERVAL_MINUTES, Math.min(...intervals));
}
