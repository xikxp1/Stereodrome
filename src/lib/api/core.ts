import { invoke } from "@tauri-apps/api/core";

import type {
  CoreCommand,
  CoreCommandValue,
} from "$lib/types/protocol.generated";

type ValueCommand = keyof CoreCommandValue;

/**
 * Sends a command to the shared runtime through the desktop shell.
 *
 * Commands listed in the generated `CoreCommandValue` resolve to their payload
 * type; every other command resolves to `void`.
 */
export function dispatch<T extends ValueCommand>(
  command: Extract<CoreCommand, { type: T }>
): Promise<CoreCommandValue[T]>;
export function dispatch(
  command: Exclude<CoreCommand, { type: ValueCommand }>
): Promise<void>;
export function dispatch(command: CoreCommand): Promise<unknown> {
  return invoke("core_dispatch", { command });
}
