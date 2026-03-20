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

  isConnecting = $state(false);
  isRestoring = $state(false);
  error = $state<string | null>(null);

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
    try {
      this.applyStatus(await getConnectionStatus());
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    }
  }

  async restore(): Promise<boolean> {
    this.isRestoring = true;
    this.error = null;

    try {
      this.applyStatus(await restoreSession());
      return this.status.connected;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
      return false;
    } finally {
      this.isRestoring = false;
    }
  }
}

export const connection = new ConnectionStore();
