/* eslint-disable */
// @generated from crates/stereodrome-core/src/protocol.rs. Do not hand-edit.

import type {
  ConnectivitySettings,
  LibrarySyncStatus,
  QueueState,
} from "@/types/music";

export const CORE_PROTOCOL_VERSION = 1 as const;

export type RuntimeLifecycle =
  | "starting"
  | "ready"
  | "shutting-down"
  | "faulted";

export type PlatformLifecycle = "foreground" | "background";

export type ConnectivityState =
  | { status: "unconfigured" }
  | {
      status: "offline-manual";
      server_url: string | null;
      username: string | null;
    }
  | { status: "disconnected"; server_url: string; username: string }
  | {
      status: "online";
      server_url: string;
      username: string;
      server_version: string | null;
    };

export type PlaybackProjectionSong = {
  id: string;
  title: string;
  artist: string;
  album: string;
  duration_seconds: number;
  artwork_uri: string | null;
};

export type PlaybackProjection = {
  state: "playing" | "paused" | "stopped" | "stalled";
  is_playing: boolean;
  audio_loaded: boolean;
  output_state: "closed" | "ready" | "failed" | "unavailable";
  song: PlaybackProjectionSong | null;
  position_seconds: number;
  duration_seconds: number;
  volume: number;
  queue: QueueState;
  queue_index: number | null;
  queue_length: number;
  can_play: boolean;
  can_next: boolean;
  can_previous: boolean;
  can_seek: boolean;
  preparing_operation_id: number | null;
};

export type ProtocolError = {
  code:
    | "unsupported_protocol_version"
    | "invalid_command_id"
    | "invalid_input"
    | "not_connected"
    | "offline_mode"
    | "conflict"
    | "runtime_unavailable"
    | "cancelled"
    | "persistence"
    | "network"
    | "audio"
    | "internal";
  message: string;
  retryable: boolean;
};

export type OperationFailure = {
  command_id: number;
  operation_id: number | null;
  error: ProtocolError;
};

export type CoreSnapshot = {
  protocol_version: typeof CORE_PROTOCOL_VERSION;
  revision: number;
  lifecycle: RuntimeLifecycle;
  connectivity: ConnectivityState;
  playback: PlaybackProjection;
  queue: QueueState;
  sync: LibrarySyncStatus;
  downloads: {
    downloading_song_ids: string[];
    offline_song_ids: string[];
  };
  operations: Array<{
    operation_id: number;
    cause_command_id: number;
    kind: unknown;
    phase: "running" | "cancelling";
  }>;
  saved_playlist_offline: {
    running: boolean;
    operation_id: number | null;
    last_error: string | null;
  };
  platform_lifecycle: PlatformLifecycle;
  network_available: boolean;
  settings_revision: number;
  library_revision: number;
  last_failure: OperationFailure | null;
};

export type CoreCommand =
  | { type: "initialize" }
  | { type: "get-snapshot" }
  | {
      type: "connect";
      params: { url: string; username: string; password: string };
    }
  | {
      type: "update-server-settings";
      update: { url?: string; username?: string };
    }
  | { type: "restore-session" }
  | { type: "disconnect" }
  | { type: "set-connectivity"; settings: ConnectivitySettings }
  | { type: "report-network"; available: boolean }
  | { type: "report-lifecycle"; lifecycle: PlatformLifecycle }
  | { type: "start-sync"; kind: "full" | "incremental" | "full-reconcile" }
  | { type: "run-background-tick" }
  | { type: "play-selection"; song_id: string; song_ids: string[] }
  | { type: "clear-playback" }
  | {
      type: "navigate-playback";
      navigation:
        | { type: "index"; index: number }
        | { type: "next"; force: boolean }
        | { type: "previous" };
    }
  | { type: "toggle-playback" }
  | { type: "seek-by"; seconds: number }
  | { type: "remove-from-queue"; index: number }
  | { type: "toggle-shuffle" }
  | { type: "cycle-repeat-mode" }
  | { type: "reroll-next" };

export type CoreCommandRequest = {
  protocol_version: typeof CORE_PROTOCOL_VERSION;
  command_id: number;
  command: CoreCommand;
};

export type CoreCommandResult<T = unknown> = {
  protocol_version: typeof CORE_PROTOCOL_VERSION;
  command_id: number;
  accepted_revision: number;
  operation_id: number | null;
  status: "succeeded" | "failed";
  value?: T;
  error?: ProtocolError;
};

export type CoreEvent = {
  protocol_version: typeof CORE_PROTOCOL_VERSION;
  stream_id: number;
  event_id: number;
  revision: number;
  cause_command_id: number;
  operation_id: number | null;
  kind:
    | { type: "snapshot-changed"; snapshot: CoreSnapshot }
    | { type: "operation-failed"; failure: OperationFailure }
    | { type: "runtime-shutting-down" };
};
