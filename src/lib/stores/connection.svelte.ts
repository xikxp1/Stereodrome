import {
  connectServer,
  disconnectServer,
  getConnectionStatus,
} from "$lib/api/commands";
import type { ConnectionStatus, ConnectParams } from "$lib/types";

class ConnectionStore {
  status = $state<ConnectionStatus>({
    connected: false,
    server_url: null,
    username: null,
    server_version: null,
  });

  isConnecting = $state(false);
  error = $state<string | null>(null);

  async connect(params: ConnectParams): Promise<boolean> {
    this.isConnecting = true;
    this.error = null;

    try {
      this.status = await connectServer(params);
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
      this.status = {
        connected: false,
        server_url: null,
        username: null,
        server_version: null,
      };
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    }
  }

  async checkStatus(): Promise<void> {
    try {
      this.status = await getConnectionStatus();
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    }
  }
}

export const connection = new ConnectionStore();
