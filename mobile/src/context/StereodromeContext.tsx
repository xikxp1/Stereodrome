import { createContext, useContext, useEffect, useMemo, useState } from "react";

import { stereodromeCore } from "@/services/stereodromeCore";
import type { ConnectionStatus } from "@/types/music";

type StereodromeContextValue = {
  ready: boolean;
  status: ConnectionStatus;
  hasConfiguredServer: boolean;
  offlineMode: boolean;
  offlineSongIds: Set<string>;
  error: string | null;
  refreshStatus(): Promise<void>;
  refreshOfflineSongIds(): Promise<void>;
  connect(params: {
    url: string;
    username: string;
    password: string;
  }): Promise<void>;
  updateServerSettings(params: {
    url?: string;
    username?: string;
  }): Promise<void>;
  sync(): Promise<void>;
  syncIncremental(): Promise<void>;
};

const disconnected: ConnectionStatus = {
  connected: false,
  server_url: null,
  username: null,
  server_version: null,
};

const StereodromeContext = createContext<StereodromeContextValue | null>(null);

export function StereodromeProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const [ready, setReady] = useState(false);
  const [status, setStatus] = useState(disconnected);
  const [offlineSongIds, setOfflineSongIds] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const hasConfiguredServer = Boolean(status.server_url);
  const offlineMode = hasConfiguredServer && !status.connected;

  async function refreshOfflineSongIds() {
    try {
      const songIds = await stereodromeCore.getOfflineSongIds();
      setOfflineSongIds(new Set(songIds));
    } catch (e) {
      setOfflineSongIds(new Set());
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function refreshStatus() {
    try {
      const next = await stereodromeCore.restoreSession();
      setStatus(next);
      setError(null);
      if (next.server_url) {
        await refreshOfflineSongIds();
      } else {
        setOfflineSongIds(new Set());
      }
    } catch (e) {
      setStatus(disconnected);
      setOfflineSongIds(new Set());
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function connect(params: {
    url: string;
    username: string;
    password: string;
  }) {
    const next = await stereodromeCore.connectServer(params);
    setStatus(next);
    setError(null);
    await refreshOfflineSongIds();
  }

  async function updateServerSettings(params: {
    url?: string;
    username?: string;
  }) {
    const next = await stereodromeCore.updateServerSettings(params);
    setStatus(next);
    setError(null);
    await refreshOfflineSongIds();
  }

  async function sync() {
    await stereodromeCore.syncLibrary();
    await refreshOfflineSongIds();
  }

  async function syncIncremental() {
    await stereodromeCore.syncLibraryIncremental();
    await refreshOfflineSongIds();
  }

  useEffect(() => {
    let mounted = true;
    stereodromeCore
      .initialize()
      .then(() => refreshStatus())
      .catch((e) => {
        if (mounted) {
          setError(e instanceof Error ? e.message : String(e));
        }
      })
      .finally(() => {
        if (mounted) {
          setReady(true);
        }
      });
    return () => {
      mounted = false;
    };
  }, []);

  const value = useMemo(
    () => ({
      ready,
      status,
      hasConfiguredServer,
      offlineMode,
      offlineSongIds,
      error,
      refreshStatus,
      refreshOfflineSongIds,
      connect,
      updateServerSettings,
      sync,
      syncIncremental,
    }),
    [error, hasConfiguredServer, offlineMode, offlineSongIds, ready, status]
  );

  return (
    <StereodromeContext.Provider value={value}>
      {children}
    </StereodromeContext.Provider>
  );
}

export function useStereodrome() {
  const value = useContext(StereodromeContext);
  if (!value) {
    throw new Error("useStereodrome must be used inside StereodromeProvider");
  }
  return value;
}
