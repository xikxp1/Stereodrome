import { useQueryClient } from "@tanstack/react-query";
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
import {
  isLibrarySyncStatus,
  isPlaybackSnapshot,
  stereodromeCore,
} from "@/services/stereodromeCore";
import type { ConnectionStatus, LibrarySyncStatus } from "@/types/music";

type StereodromeContextValue = {
  ready: boolean;
  status: ConnectionStatus;
  hasConfiguredServer: boolean;
  offlineMode: boolean;
  manualOfflineEnabled: boolean;
  offlineSongIds: Set<string>;
  downloadingSongIds: Set<string>;
  error: string | null;
  refreshStatus(): Promise<void>;
  refreshOfflineSongIds(): Promise<void>;
  reconcileSavedPlaylistsOffline(): Promise<void>;
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

const disconnected: ConnectionStatus = {
  connected: false,
  server_url: null,
  username: null,
  server_version: null,
};

const librarySyncStatusQueryKey = ["library-sync-status"] as const;
const savedPlaylistOfflinePollIntervalMs = 2000;
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
  const savedPlaylistOfflinePoll = useRef<ReturnType<
    typeof setInterval
  > | null>(null);
  const hasConfiguredServer = Boolean(status.server_url);
  const offlineMode =
    manualOfflineEnabled || (hasConfiguredServer && !status.connected);

  const refreshOfflineSongIds = useCallback(async () => {
    try {
      const songIds = await stereodromeCore.getOfflineSongIds();
      setOfflineSongIds(new Set(songIds));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const stopSavedPlaylistOfflinePolling = useCallback(() => {
    if (savedPlaylistOfflinePoll.current === null) {
      return;
    }
    clearInterval(savedPlaylistOfflinePoll.current);
    savedPlaylistOfflinePoll.current = null;
  }, []);

  const pollSavedPlaylistOfflineReconcile = useCallback(async () => {
    try {
      const reconcileStatus =
        await stereodromeCore.getSavedPlaylistsOfflineReconcileStatus();
      if (reconcileStatus.running) {
        return;
      }

      stopSavedPlaylistOfflinePolling();
      if (
        reconcileStatus.last_error !== null &&
        reconcileStatus.last_error.length > 0
      ) {
        setError(reconcileStatus.last_error);
      } else {
        setError(null);
      }
      await refreshOfflineSongIds();
    } catch (e) {
      stopSavedPlaylistOfflinePolling();
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [refreshOfflineSongIds, stopSavedPlaylistOfflinePolling]);

  const startSavedPlaylistOfflinePolling = useCallback(() => {
    if (savedPlaylistOfflinePoll.current !== null) {
      return;
    }
    pollSavedPlaylistOfflineReconcile().catch((pollError: unknown) => {
      setError(
        pollError instanceof Error ? pollError.message : String(pollError)
      );
    });
    savedPlaylistOfflinePoll.current = setInterval(() => {
      pollSavedPlaylistOfflineReconcile().catch((pollError: unknown) => {
        setError(
          pollError instanceof Error ? pollError.message : String(pollError)
        );
      });
    }, savedPlaylistOfflinePollIntervalMs);
  }, [pollSavedPlaylistOfflineReconcile]);

  const reconcileSavedPlaylistsOfflineInBackground = useCallback(async () => {
    await stereodromeCore.startSavedPlaylistsOfflineReconcile();
    startSavedPlaylistOfflinePolling();
  }, [startSavedPlaylistOfflinePolling]);

  const refreshStatus = useCallback(async () => {
    try {
      const connectivitySettings =
        await stereodromeCore.getConnectivitySettings();
      setManualOfflineEnabledState(connectivitySettings.manual_offline_enabled);
      const next = await stereodromeCore.restoreSession();
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
      setStatus(disconnected);
      setOfflineSongIds(new Set());
      setDownloadingSongIds(new Set());
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [reconcileSavedPlaylistsOfflineInBackground, refreshOfflineSongIds]);

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
        queryClient.invalidateQueries({ queryKey })
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
      stopSavedPlaylistOfflinePolling();
    };
  }, [refreshStatus, stopSavedPlaylistOfflinePolling]);

  useEffect(
    () =>
      stereodromeCore.addEventListener(
        "playback-snapshot",
        (snapshot) => {
          setOfflineSongIds(new Set(snapshot.downloaded_song_ids));
          setDownloadingSongIds(new Set(snapshot.downloading_song_ids));
        },
        isPlaybackSnapshot
      ),
    []
  );

  useEffect(
    () =>
      stereodromeCore.addEventListener(
        "sync-status-changed",
        (syncStatus) => {
          queryClient.setQueryData(librarySyncStatusQueryKey, syncStatus);
          lastCompletedSyncKey.current = completedSyncKey(syncStatus);

          const wasActive = syncWasActive.current;
          const isActive =
            syncStatus.active_job !== null && syncStatus.active_job.length > 0;
          syncWasActive.current = isActive;

          if (wasActive && !isActive) {
            refreshLibraryAfterSync().catch((refreshError: unknown) => {
              setError(
                refreshError instanceof Error
                  ? refreshError.message
                  : String(refreshError)
              );
            });
          }
        },
        isLibrarySyncStatus
      ),
    [queryClient, refreshLibraryAfterSync]
  );

  useEffect(() => {
    function handleAppStateChange(nextState: AppStateStatus) {
      if (nextState === "active") {
        refreshSyncStatusAfterForeground().catch((refreshError: unknown) => {
          setError(
            refreshError instanceof Error
              ? refreshError.message
              : String(refreshError)
          );
        });
        if (!manualOfflineEnabled) {
          syncLibraryBackgroundRegistration().catch(
            (registrationError: unknown) => {
              setError(
                registrationError instanceof Error
                  ? registrationError.message
                  : String(registrationError)
              );
            }
          );
        }
      }
    }

    const subscription = AppState.addEventListener(
      "change",
      handleAppStateChange
    );
    return () => {
      subscription.remove();
    };
  }, [manualOfflineEnabled, refreshSyncStatusAfterForeground]);

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
      offlineSongIds,
      downloadingSongIds,
      error,
      refreshStatus,
      refreshOfflineSongIds,
      reconcileSavedPlaylistsOffline:
        reconcileSavedPlaylistsOfflineInBackground,
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
      offlineSongIds,
      downloadingSongIds,
      connect,
      reconcileSavedPlaylistsOfflineInBackground,
      ready,
      refreshOfflineSongIds,
      refreshStatus,
      setManualOfflineEnabled,
      status,
      sync,
      syncIncremental,
      updateServerSettings,
    ]
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

function completedSyncKey(syncStatus: LibrarySyncStatus): string {
  return [
    syncStatus.incremental.last_success_at ?? "",
    syncStatus.full_reconcile.last_success_at ?? "",
  ].join("|");
}
