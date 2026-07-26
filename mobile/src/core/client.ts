import { Directory, Paths } from "expo-file-system";

import NativeStereodromeCore from "../../modules/stereodrome-core/src";
import {
  CORE_PROTOCOL_VERSION,
  type CoreCommand,
  type CoreCommandValue,
  type CoreCommandRequest,
  type CoreEvent,
  type OperationFailure,
  type ProtocolError,
  type CoreSnapshot,
} from "@/core/protocol.generated";

const unavailable =
  "Stereodrome native core is not available in this development build";

let initializePromise: Promise<void> | null = null;
let nextCommandId = Math.floor(Date.now() * 1000);
let nativeSubscription: { remove(): void } | null = null;
const eventListeners = new Set<(event: CoreEvent) => void>();
const errorListeners = new Set<(error: unknown) => void>();

function fileUriToPath(uri: string): string {
  return uri.startsWith("file://")
    ? decodeURIComponent(uri.replace(/^file:\/\//, ""))
    : uri;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNullableNumber(value: unknown): value is number | null {
  return value === null || typeof value === "number";
}

function isProtocolError(value: unknown): value is ProtocolError {
  return (
    isRecord(value) &&
    typeof value["code"] === "string" &&
    typeof value["message"] === "string" &&
    typeof value["retryable"] === "boolean"
  );
}

function isOperationFailure(value: unknown): value is OperationFailure {
  return (
    isRecord(value) &&
    typeof value["command_id"] === "number" &&
    isNullableNumber(value["operation_id"]) &&
    isProtocolError(value["error"])
  );
}

function isCoreSnapshot(value: unknown): value is CoreSnapshot {
  return (
    isRecord(value) &&
    value["protocol_version"] === CORE_PROTOCOL_VERSION &&
    typeof value["revision"] === "number" &&
    isRecord(value["connectivity"]) &&
    isRecord(value["playback"]) &&
    isRecord(value["queue"]) &&
    isRecord(value["downloads"]) &&
    typeof value["library_revision"] === "number"
  );
}

function parseRuntimeEvent(raw: string): CoreEvent {
  const event: unknown = JSON.parse(raw);
  if (
    !isRecord(event) ||
    event["protocol_version"] !== CORE_PROTOCOL_VERSION ||
    typeof event["stream_id"] !== "number" ||
    typeof event["event_id"] !== "number" ||
    typeof event["revision"] !== "number" ||
    typeof event["cause_command_id"] !== "number" ||
    !isNullableNumber(event["operation_id"]) ||
    !isRecord(event["kind"]) ||
    typeof event["kind"]["type"] !== "string"
  ) {
    throw new Error("Native core emitted an invalid runtime event");
  }
  const base = {
    protocol_version: CORE_PROTOCOL_VERSION,
    stream_id: event["stream_id"],
    event_id: event["event_id"],
    revision: event["revision"],
    cause_command_id: event["cause_command_id"],
    operation_id: event["operation_id"],
  };
  switch (event["kind"]["type"]) {
    case "snapshot-changed":
      if (!isCoreSnapshot(event["kind"]["snapshot"])) {
        throw new Error("Native core emitted an invalid runtime snapshot");
      }
      return {
        ...base,
        kind: {
          type: "snapshot-changed",
          snapshot: event["kind"]["snapshot"],
        },
      };
    case "operation-failed":
      if (!isOperationFailure(event["kind"]["failure"])) {
        throw new Error("Native core emitted an invalid operation failure");
      }
      return {
        ...base,
        kind: {
          type: "operation-failed",
          failure: event["kind"]["failure"],
        },
      };
    case "runtime-shutting-down":
      return { ...base, kind: { type: "runtime-shutting-down" } };
    default:
      throw new Error("Native core emitted an unknown runtime event");
  }
}

function startEventSubscription(): void {
  if (
    nativeSubscription !== null ||
    NativeStereodromeCore.addListener === undefined
  ) {
    return;
  }
  nativeSubscription = NativeStereodromeCore.addListener(
    "core-event",
    ({ event }) => {
      try {
        const parsed = parseRuntimeEvent(event);
        eventListeners.forEach((listener) => {
          listener(parsed);
        });
      } catch (error) {
        errorListeners.forEach((listener) => {
          listener(error);
        });
      }
    }
  );
}

async function initialize(): Promise<void> {
  if (initializePromise !== null) {
    return initializePromise;
  }
  initializePromise = (async () => {
    if (NativeStereodromeCore.initialize === undefined) {
      throw new Error(unavailable);
    }
    startEventSubscription();
    const dataDir = new Directory(Paths.document, "stereodrome");
    dataDir.create({ idempotent: true, intermediates: true });
    const initialized = await NativeStereodromeCore.initialize(
      fileUriToPath(dataDir.uri)
    );
    if (!initialized) {
      throw new Error("Stereodrome Rust core failed to initialize");
    }
  })();

  try {
    await initializePromise;
  } catch (error) {
    initializePromise = null;
    throw error;
  }
}

async function dispatch(command: CoreCommand): Promise<unknown> {
  await initialize();
  if (NativeStereodromeCore.dispatch === undefined) {
    throw new Error(unavailable);
  }

  const commandId = nextCommandId++;
  const request: CoreCommandRequest = {
    protocol_version: CORE_PROTOCOL_VERSION,
    command_id: commandId,
    command,
  };
  const parsed: unknown = JSON.parse(
    await NativeStereodromeCore.dispatch(JSON.stringify(request))
  );
  if (
    !isRecord(parsed) ||
    parsed["protocol_version"] !== CORE_PROTOCOL_VERSION ||
    parsed["command_id"] !== commandId ||
    (parsed["status"] !== "succeeded" && parsed["status"] !== "failed")
  ) {
    throw new Error("Native core returned an invalid runtime result");
  }

  if (parsed["status"] === "failed") {
    const error = parsed["error"];
    throw new Error(
      isProtocolError(error) ? error.message : "Runtime command failed"
    );
  }
  return parsed["value"];
}

type CommandType = keyof CoreCommandValue;

function dispatchTyped<T extends CommandType>(
  command: Extract<CoreCommand, { type: T }>
): Promise<CoreCommandValue[T]>;
function dispatchTyped(command: CoreCommand): Promise<unknown>;
function dispatchTyped(command: CoreCommand): Promise<unknown> {
  return dispatch(command);
}

async function dispatchSnapshot(
  command: Extract<CoreCommand, { type: "initialize" | "get-snapshot" }>
): Promise<CoreSnapshot> {
  const snapshot = await dispatch(command);
  if (!isCoreSnapshot(snapshot)) {
    throw new Error("Native core returned an invalid runtime snapshot");
  }
  return snapshot;
}

export const coreClient = {
  initialize,
  dispatch,
  dispatchTyped,
  initializeSnapshot(): Promise<CoreSnapshot> {
    return dispatchSnapshot({ type: "initialize" });
  },
  snapshot(): Promise<CoreSnapshot> {
    return dispatchSnapshot({ type: "get-snapshot" });
  },
  subscribe(listener: (event: CoreEvent) => void): () => void {
    startEventSubscription();
    eventListeners.add(listener);
    return () => {
      eventListeners.delete(listener);
    };
  },
  subscribeErrors(listener: (error: unknown) => void): () => void {
    errorListeners.add(listener);
    return () => {
      errorListeners.delete(listener);
    };
  },
};
