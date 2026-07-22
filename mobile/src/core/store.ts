import { coreClient } from "@/core/client";
import type { CoreEvent, CoreSnapshot } from "@/core/protocol.generated";
import type { PlayableSong } from "@/types/music";

export type CoreStoreState = {
  ready: boolean;
  snapshot: CoreSnapshot | null;
  error: string | null;
};

const listeners = new Set<() => void>();
let state: CoreStoreState = { ready: false, snapshot: null, error: null };
let initializePromise: Promise<void> | null = null;
let subscriptionsStarted = false;
let eventStreamId: number | null = null;
let lastEventId = 0;

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function publish(next: CoreStoreState): void {
  if (next === state) {
    return;
  }
  state = next;
  listeners.forEach((listener) => {
    listener();
  });
}

function setError(error: unknown): void {
  const message = errorMessage(error);
  if (state.error !== message) {
    publish({ ...state, error: message });
  }
}

function applySnapshot(snapshot: CoreSnapshot): void {
  if (state.snapshot !== null && snapshot.revision < state.snapshot.revision) {
    return;
  }
  if (
    state.snapshot !== null &&
    snapshot.revision === state.snapshot.revision &&
    JSON.stringify(snapshot) === JSON.stringify(state.snapshot)
  ) {
    return;
  }
  publish({ ...state, snapshot });
}

function applyEvent(event: CoreEvent): void {
  if (eventStreamId !== event.stream_id) {
    eventStreamId = event.stream_id;
    lastEventId = 0;
  }
  if (event.event_id <= lastEventId) {
    return;
  }
  lastEventId = event.event_id;

  switch (event.kind.type) {
    case "snapshot-changed":
      applySnapshot(event.kind.snapshot);
      break;
    case "operation-failed":
      publish({ ...state, error: event.kind.failure.error.message });
      break;
    case "runtime-shutting-down":
      publish({ ...state, ready: false });
      break;
  }
}

function startSubscriptions(): void {
  if (subscriptionsStarted) {
    return;
  }
  subscriptionsStarted = true;
  coreClient.subscribe(applyEvent);
  coreClient.subscribeErrors(setError);
}

async function reconcile(): Promise<void> {
  try {
    applySnapshot(await coreClient.snapshot());
  } catch (error) {
    setError(error);
    throw error;
  }
}

async function initialize(): Promise<void> {
  if (initializePromise !== null) {
    return initializePromise;
  }
  startSubscriptions();
  initializePromise = (async () => {
    try {
      await coreClient.initialize();
      const initial = await coreClient.initializeSnapshot();
      applySnapshot(initial);

      if (initial.connectivity.status !== "offline-manual") {
        try {
          await coreClient.dispatch({ type: "restore-session" });
        } catch (error) {
          setError(error);
        }
      }
      await reconcile();
    } catch (error) {
      setError(error);
    } finally {
      publish({ ...state, ready: true });
    }
  })();
  return initializePromise;
}

async function run(
  command: Parameters<typeof coreClient.dispatch>[0],
  clearError = true
) {
  if (clearError && state.error !== null) {
    publish({ ...state, error: null });
  }
  try {
    return await coreClient.dispatch(command);
  } catch (error) {
    setError(error);
    throw error;
  }
}

export const coreActions = {
  async connect(params: { url: string; username: string; password: string }) {
    await run({ type: "connect", params });
  },
  async updateServerSettings(update: { url?: string; username?: string }) {
    await run({ type: "update-server-settings", update });
  },
  async disconnect() {
    await run({ type: "disconnect" });
  },
  async setManualOfflineEnabled(enabled: boolean) {
    await run({
      type: "set-connectivity",
      settings: { manual_offline_enabled: enabled },
    });
  },
  async sync(kind: "full" | "incremental") {
    await run({ type: "start-sync", kind });
  },
  async playSong(song: PlayableSong, queue: PlayableSong[] = [song]) {
    await run({
      type: "play-selection",
      song_id: song.id,
      song_ids: queue.map((candidate) => candidate.id),
    });
  },
  async togglePlayback() {
    await run({ type: "toggle-playback" });
  },
  async navigate(
    navigation:
      | { type: "index"; index: number }
      | { type: "next"; force: boolean }
      | { type: "previous" }
  ) {
    await run({ type: "navigate-playback", navigation });
  },
  async seekBy(seconds: number) {
    if (Number.isFinite(seconds) && seconds !== 0) {
      await run({ type: "seek-by", seconds });
    }
  },
  async removeQueueIndex(index: number) {
    await run({ type: "remove-from-queue", index });
  },
  async clearPlayback() {
    await run({ type: "clear-playback" });
  },
  async toggleShuffle() {
    await run({ type: "toggle-shuffle" });
  },
  async cycleRepeatMode() {
    await run({ type: "cycle-repeat-mode" });
  },
  async rerollNext() {
    await run({ type: "reroll-next" });
  },
  async reportLifecycle(lifecycle: "foreground" | "background") {
    await run({ type: "report-lifecycle", lifecycle }, false);
  },
  async reportNetwork(available: boolean) {
    await run({ type: "report-network", available }, false);
  },
  async runBackgroundTick() {
    await run({ type: "run-background-tick" }, false);
  },
  reconcile,
};

export const coreStore = {
  initialize,
  getState(): CoreStoreState {
    return state;
  },
  subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
};
