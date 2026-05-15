import { createContext, useContext, useEffect, useMemo, useState } from "react";

import { stereodromeCore } from "@/services/stereodromeCore";
import type { ConnectionStatus } from "@/types/music";

type StereodromeContextValue = {
  ready: boolean;
  status: ConnectionStatus;
  error: string | null;
  refreshStatus(): Promise<void>;
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
  reconcile(): Promise<void>;
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
  const [error, setError] = useState<string | null>(null);

  async function refreshStatus() {
    try {
      const next = await stereodromeCore.restoreSession();
      setStatus(next);
      setError(null);
    } catch (e) {
      setStatus(disconnected);
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
  }

  async function updateServerSettings(params: {
    url?: string;
    username?: string;
  }) {
    const next = await stereodromeCore.updateServerSettings(params);
    setStatus(next);
    setError(null);
  }

  async function sync() {
    await stereodromeCore.syncLibrary();
  }

  async function syncIncremental() {
    await stereodromeCore.syncLibraryIncremental();
  }

  async function reconcile() {
    await stereodromeCore.reconcileLibrary();
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
      error,
      refreshStatus,
      connect,
      updateServerSettings,
      sync,
      syncIncremental,
      reconcile,
    }),
    [error, ready, status]
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
