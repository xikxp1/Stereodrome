import { useQueryClient } from "@tanstack/react-query";
import * as Network from "expo-network";
import { AppState, type AppStateStatus } from "react-native";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { syncLibraryBackgroundRegistration } from "@/services/librarySyncScheduler";
import { stereodromeCore } from "@/services/stereodromeCore";
import type {
  ConnectionStatus,
  FileStateSnapshot,
  LibrarySyncStatus,
} from "@/types/music";

type StereodromeContextValue = {
  ready: boolean;
  status: ConnectionStatus;
  hasConfiguredServer: boolean;
  offlineMode: boolean;
  manualOfflineEnabled: boolean;
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
  setManualOfflineEnabled(enabled: boolean): Promise<void>;
  sync(): Promise<void>;
  syncIncremental(): Promise<void>;
};

type FileStateContextValue = {
  offlineSongIds: Set<string>;
  downloadingSongIds: Set<string>;
  refreshOfflineSongIds(): Promise<void>;
  reconcileSavedPlaylistsOffline(): Promise<void>;
};

const disconnected: ConnectionStatus = {
  connected: false,
  server_url: null,
  username: null,
  server_version: null,
};

const librarySyncStatusQueryKey = ["library-sync-status"] as const;
const libraryQueryKeys = [
  ["artists"],
  ["albums"],
  ["songs"],
  ["artist-albums"],
  ["artist-songs"],
  ["album-songs"],
  ["album-list"],
  ["search"],
  ["playlists"],
  ["playlist-songs"],
] as const;

const StereodromeContext = createContext<StereodromeContextValue | null>(null);
const FileStateContext = createContext<FileStateContextValue | null>(null);

function setsEqual(current: Set<string>, values: readonly string[]): boolean {
  return (
    current.size === values.length &&
    values.every((value) => current.has(value))
  );
}

function updateStringSet(
  current: Set<string>,
  values: readonly string[]
): Set<string> {
  return setsEqual(current, values) ? current : new Set(values);
}

export function StereodromeProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const queryClient = useQueryClient();
  const [ready, setReady] = useState(false);
  const [status, setStatus] = useState(disconnected);
  const [manualOfflineEnabled, setManualOfflineEnabledState] = useState(false);
  const [offlineSongIds, setOfflineSongIds] = useState<Set<string>>(new Set());
  const [downloadingSongIds, setDownloadingSongIds] = useState<Set<string>>(
    new Set()
  );
  const [error, setError] = useState<string | null>(null);
  const syncWasActive = useRef(false);
  const lastCompletedSyncKey = useRef<string | null>(null);
  const statusRefreshGeneration = useRef(0);
  const appState = useRef(AppState.currentState);
  const pendingLibraryRefresh = useRef(false);
  const pendingOfflineRefresh = useRef(false);
  const lastFileStateSeq = useRef(-1);
  const hasConfiguredServer = Boolean(status.server_url);
  const offlineMode =
    manualOfflineEnabled || (hasConfiguredServer && !status.connected);

  const applyFileState = useCallback((fileState: FileStateSnapshot) => {
    if (fileState.seq <= lastFileStateSeq.current) {
      return;
    }
    lastFileStateSeq.current = fileState.seq;
    setOfflineSongIds((current) =>
      updateStringSet(current, fileState.downloaded_song_ids)
    );
    setDownloadingSongIds((current) =>
      updateStringSet(current, fileState.downloading_song_ids)
    );
  }, []);

  const refreshOfflineSongIds = useCallback(async () => {
    try {
      applyFileState(await stereodromeCore.getFileStateSnapshot());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [applyFileState]);

  const reconcileSavedPlaylistsOfflineInBackground = useCallback(async () => {
    await stereodromeCore.startSavedPlaylistsOfflineReconcile();
  }, []);

  const refreshStatus = useCallback(
    async (restore = true) => {
      const generation = ++statusRefreshGeneration.current;
      try {
        const connectivitySettings =
          await stereodromeCore.getConnectivitySettings();
        if (generation !== statusRefreshGeneration.current) {
          return;
        }
        setManualOfflineEnabledState(
          connectivitySettings.manual_offline_enabled
        );
        const next = restore
          ? await stereodromeCore.restoreSession()
          : await stereodromeCore.getConnectionStatus();
        if (generation !== statusRefreshGeneration.current) {
          return;
        }
        setStatus(next);
        setError(null);
        if (next.server_url !== null && next.server_url.length > 0) {
          await refreshOfflineSongIds();
          if (next.connected && !connectivitySettings.manual_offline_enabled) {
            reconcileSavedPlaylistsOfflineInBackground().catch(
              (reconcileError: unknown) => {
                setError(
                  reconcileError instanceof Error
                    ? reconcileError.message
                    : String(reconcileError)
                );
              }
            );
          }
        } else {
          setOfflineSongIds(new Set());
          setDownloadingSongIds(new Set());
        }
      } catch (e) {
        if (generation !== statusRefreshGeneration.current) {
          return;
        }
        setStatus(disconnected);
        setOfflineSongIds(new Set());
        setDownloadingSongIds(new Set());
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [reconcileSavedPlaylistsOfflineInBackground, refreshOfflineSongIds]
  );

  const refreshStatusForNetworkState = useCallback(
    async (networkState: Network.NetworkState) => {
      const unavailable =
        networkState.isConnected === false ||
        networkState.isInternetReachable === false;

      if (unavailable) {
        ++statusRefreshGeneration.current;
        setStatus((current) => ({
          ...current,
          connected: false,
          server_version: null,
        }));
      }
      await stereodromeCore.reportNetwork(!unavailable);
      await refreshStatus(false);
    },
    [refreshStatus]
  );

  const connect = useCallback(
    async (params: { url: string; username: string; password: string }) => {
      if (manualOfflineEnabled) {
        throw new Error("Offline mode is enabled");
      }

      const next = await stereodromeCore.connectServer(params);
      setStatus(next);
      setError(null);
      await reconcileSavedPlaylistsOfflineInBackground();
      await refreshOfflineSongIds();
    },
    [
      manualOfflineEnabled,
      reconcileSavedPlaylistsOfflineInBackground,
      refreshOfflineSongIds,
    ]
  );

  const updateServerSettings = useCallback(
    async (params: { url?: string; username?: string }) => {
      if (manualOfflineEnabled) {
        throw new Error("Offline mode is enabled");
      }

      const next = await stereodromeCore.updateServerSettings(params);
      setStatus(next);
      setError(null);
      if (next.connected) {
        await reconcileSavedPlaylistsOfflineInBackground();
      }
      await refreshOfflineSongIds();
    },
    [
      manualOfflineEnabled,
      reconcileSavedPlaylistsOfflineInBackground,
      refreshOfflineSongIds,
    ]
  );

  const sync = useCallback(async () => {
    if (manualOfflineEnabled) {
      throw new Error("Offline mode is enabled");
    }

    syncWasActive.current = true;
    try {
      await stereodromeCore.syncLibrary();
    } catch (e) {
      syncWasActive.current = false;
      throw e;
    }
  }, [manualOfflineEnabled]);

  const syncIncremental = useCallback(async () => {
    if (manualOfflineEnabled) {
      throw new Error("Offline mode is enabled");
    }

    syncWasActive.current = true;
    try {
      await stereodromeCore.syncLibraryIncremental();
    } catch (e) {
      syncWasActive.current = false;
      throw e;
    }
  }, [manualOfflineEnabled]);

  const refreshLibraryAfterSync = useCallback(async () => {
    await Promise.all(
      libraryQueryKeys.map((queryKey) =>
        queryClient.invalidateQueries({ queryKey, refetchType: "all" })
      )
    );
    if (status.connected && !manualOfflineEnabled) {
      await reconcileSavedPlaylistsOfflineInBackground();
    }
    await refreshOfflineSongIds();
  }, [
    manualOfflineEnabled,
    queryClient,
    reconcileSavedPlaylistsOfflineInBackground,
    refreshOfflineSongIds,
    status.connected,
  ]);

  const refreshSyncStatusAfterForeground = useCallback(async () => {
    if (manualOfflineEnabled) {
      return;
    }

    try {
      const syncStatus = await stereodromeCore.getLibrarySyncStatus();
      queryClient.setQueryData(librarySyncStatusQueryKey, syncStatus);

      const nextCompletedSyncKey = completedSyncKey(syncStatus);
      if (
        lastCompletedSyncKey.current !== null &&
        lastCompletedSyncKey.current.length > 0 &&
        nextCompletedSyncKey !== lastCompletedSyncKey.current
      ) {
        await refreshLibraryAfterSync();
      }
      lastCompletedSyncKey.current = nextCompletedSyncKey;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [manualOfflineEnabled, queryClient, refreshLibraryAfterSync]);

  useEffect(() => {
    let mounted = true;
    stereodromeCore
      .initialize()
      .then(async () => {
        await refreshStatus();
        await syncLibraryBackgroundRegistration();
      })
      .catch((e: unknown) => {
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
  }, [refreshStatus]);

  useEffect(
    () =>
      stereodromeCore.addEventListener("file-state-changed", (snapshot) => {
        if (appState.current === "active") {
          applyFileState(snapshot);
        } else {
          pendingOfflineRefresh.current = true;
        }
      }),
    [applyFileState]
  );

  useEffect(
    () =>
      stereodromeCore.addEventListener(
        "saved-playlist-offline-status-changed",
        (reconcileStatus) => {
          if (reconcileStatus.running) {
            return;
          }
          setError(reconcileStatus.last_error);
          if (appState.current === "active") {
            refreshOfflineSongIds().catch((refreshError: unknown) => {
              setError(
                refreshError instanceof Error
                  ? refreshError.message
                  : String(refreshError)
              );
            });
          } else {
            pendingOfflineRefresh.current = true;
          }
        }
      ),
    [refreshOfflineSongIds]
  );

  useEffect(
    () =>
      stereodromeCore.addEventListener("sync-status-changed", (syncStatus) => {
        queryClient.setQueryData(librarySyncStatusQueryKey, syncStatus);
        lastCompletedSyncKey.current = completedSyncKey(syncStatus);

        const wasActive = syncWasActive.current;
        const isActive =
          syncStatus.active_job !== null && syncStatus.active_job.length > 0;
        syncWasActive.current = isActive;

        if (wasActive && !isActive) {
          if (appState.current !== "active") {
            pendingLibraryRefresh.current = true;
            return;
          }
          refreshLibraryAfterSync().catch((refreshError: unknown) => {
            setError(
              refreshError instanceof Error
                ? refreshError.message
                : String(refreshError)
            );
          });
        }
      }),
    [queryClient, refreshLibraryAfterSync]
  );

  useEffect(() => {
    const subscription = Network.addNetworkStateListener((networkState) => {
      refreshStatusForNetworkState(networkState).catch(
        (refreshError: unknown) => {
          setError(
            refreshError instanceof Error
              ? refreshError.message
              : String(refreshError)
          );
        }
      );
    });
    return () => {
      subscription.remove();
    };
  }, [refreshStatusForNetworkState]);

  useEffect(() => {
    function handleAppStateChange(nextState: AppStateStatus) {
      appState.current = nextState;
      stereodromeCore
        .reportLifecycle(nextState === "active" ? "foreground" : "background")
        .catch((lifecycleError: unknown) => {
          setError(
            lifecycleError instanceof Error
              ? lifecycleError.message
              : String(lifecycleError)
          );
        });
      if (nextState === "active") {
        Network.getNetworkStateAsync()
          .then(refreshStatusForNetworkState)
          .catch((refreshError: unknown) => {
            setError(
              refreshError instanceof Error
                ? refreshError.message
                : String(refreshError)
            );
          });
        refreshSyncStatusAfterForeground()
          .then(async () => {
            if (pendingLibraryRefresh.current) {
              pendingLibraryRefresh.current = false;
              await refreshLibraryAfterSync();
            }
          })
          .catch((refreshError: unknown) => {
            setError(
              refreshError instanceof Error
                ? refreshError.message
                : String(refreshError)
            );
          });
        stereodromeCore
          .getSavedPlaylistsOfflineReconcileStatus()
          .then(async (reconcileStatus) => {
            if (!reconcileStatus.running) {
              setError(reconcileStatus.last_error);
              if (pendingOfflineRefresh.current) {
                pendingOfflineRefresh.current = false;
                await refreshOfflineSongIds();
              }
            }
          })
          .catch((refreshError: unknown) => {
            setError(
              refreshError instanceof Error
                ? refreshError.message
                : String(refreshError)
            );
          });
      }
    }

    const subscription = AppState.addEventListener(
      "change",
      handleAppStateChange
    );
    return () => {
      subscription.remove();
    };
  }, [
    refreshLibraryAfterSync,
    refreshOfflineSongIds,
    refreshStatusForNetworkState,
    refreshSyncStatusAfterForeground,
  ]);

  const setManualOfflineEnabled = useCallback(
    async (enabled: boolean) => {
      const settings = await stereodromeCore.setConnectivitySettings({
        manual_offline_enabled: enabled,
      });
      setManualOfflineEnabledState(settings.manual_offline_enabled);
      await refreshStatus();
      await syncLibraryBackgroundRegistration();
    },
    [refreshStatus]
  );

  const value = useMemo(
    () => ({
      ready,
      status,
      hasConfiguredServer,
      offlineMode,
      manualOfflineEnabled,
      error,
      refreshStatus,
      connect,
      updateServerSettings,
      setManualOfflineEnabled,
      sync,
      syncIncremental,
    }),
    [
      error,
      hasConfiguredServer,
      manualOfflineEnabled,
      offlineMode,
      connect,
      ready,
      refreshStatus,
      setManualOfflineEnabled,
      status,
      sync,
      syncIncremental,
      updateServerSettings,
    ]
  );

  const fileStateValue = useMemo(
    () => ({
      offlineSongIds,
      downloadingSongIds,
      refreshOfflineSongIds,
      reconcileSavedPlaylistsOffline:
        reconcileSavedPlaylistsOfflineInBackground,
    }),
    [
      downloadingSongIds,
      offlineSongIds,
      reconcileSavedPlaylistsOfflineInBackground,
      refreshOfflineSongIds,
    ]
  );

  return (
    <StereodromeContext.Provider value={value}>
      <FileStateContext.Provider value={fileStateValue}>
        {children}
      </FileStateContext.Provider>
    </StereodromeContext.Provider>
  );
}

export function useFileState() {
  const value = useContext(FileStateContext);
  if (!value) {
    throw new Error("useFileState must be used inside StereodromeProvider");
  }
  return value;
}

export function useStereodrome() {
  const value = useContext(StereodromeContext);
  if (!value) {
    throw new Error("useStereodrome must be used inside StereodromeProvider");
  }
  return value;
}

function completedSyncKey(syncStatus: LibrarySyncStatus): string {
  return [
    syncStatus.incremental.last_success_at ?? "",
    syncStatus.full_reconcile.last_success_at ?? "",
  ].join("|");
}
