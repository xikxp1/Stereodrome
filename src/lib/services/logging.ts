import { error } from "@tauri-apps/plugin-log";

function describeError(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/** Sends an error to Tauri logging without creating an unhandled promise. */
export function logError(message: string, cause?: unknown): void {
  const detail =
    cause === undefined ? message : `${message}: ${describeError(cause)}`;
  void error(detail).catch(() => {
    // Logging is best-effort; never replace the original failure with one here.
  });
}
