import {
  connectServer,
  disconnectServer,
  getConnectionStatus,
  restoreSession,
} from "$lib/api/commands";
import type { ConnectionStatus, ConnectParams } from "$lib/types";

const EMPTY_STATUS: ConnectionStatus = {
  connected: false,
  server_url: null,
  username: null,
  server_version: null,
};

class ConnectionStore {
  status = $state<ConnectionStatus>({ ...EMPTY_STATUS });

  hasInitialized = $state(false);
  isInitializing = $state(false);
  isConnecting = $state(false);
  error = $state<string | null>(null);

  private initializePromise: Promise<boolean> | null = null;
  private checkStatusPromise: Promise<void> | null = null;

  private applyStatus(nextStatus: ConnectionStatus): void {
    const nextServerVersion =
      nextStatus.server_version ??
      (nextStatus.server_url === this.status.server_url &&
      nextStatus.username === this.status.username
        ? this.status.server_version
        : null);

    const normalizedStatus: ConnectionStatus = {
      ...nextStatus,
      server_version: nextServerVersion,
    };

    if (
      normalizedStatus.connected === this.status.connected &&
      normalizedStatus.server_url === this.status.server_url &&
      normalizedStatus.username === this.status.username &&
      normalizedStatus.server_version === this.status.server_version
    ) {
      return;
    }

    this.status = normalizedStatus;
  }

  async connect(params: ConnectParams): Promise<boolean> {
    this.isConnecting = true;
    this.error = null;

    try {
      this.applyStatus(await connectServer(params));
      this.hasInitialized = true;
      return true;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
      return false;
    } finally {
      this.isConnecting = false;
    }
  }

  async disconnect(): Promise<void> {
    try {
      await disconnectServer();
      this.applyStatus({ ...EMPTY_STATUS });
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    }
  }

  async checkStatus(): Promise<void> {
    if (this.checkStatusPromise) {
      return this.checkStatusPromise;
    }

    this.checkStatusPromise = (async () => {
      try {
        const currentStatus = await getConnectionStatus();
        this.applyStatus(
          currentStatus.server_url ? await restoreSession() : currentStatus
        );
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e);
      } finally {
        this.checkStatusPromise = null;
      }
    })();

    return this.checkStatusPromise;
  }

  async initialize(): Promise<boolean> {
    if (this.hasInitialized) {
      return this.status.connected;
    }

    if (this.initializePromise) {
      return this.initializePromise;
    }

    this.isInitializing = true;
    this.error = null;

    this.initializePromise = (async () => {
      try {
        this.applyStatus(await restoreSession());
        return this.status.connected;
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e);
        return false;
      } finally {
        this.hasInitialized = true;
        this.isInitializing = false;
        this.initializePromise = null;
      }
    })();

    return this.initializePromise;
  }
}

export const connection = new ConnectionStore();
