import { useQueryClient } from "@tanstack/react-query";
import { AppState, type AppStateStatus } from "react-native";
import {
  createContext,
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
  LibrarySyncStatus,
  PlaybackSnapshot,
} from "@/types/music";

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

  async function refreshOfflineSongIds() {
    try {
      const songIds = await stereodromeCore.getOfflineSongIds();
      setOfflineSongIds(new Set(songIds));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  function stopSavedPlaylistOfflinePolling() {
    if (!savedPlaylistOfflinePoll.current) {
      return;
    }
    clearInterval(savedPlaylistOfflinePoll.current);
    savedPlaylistOfflinePoll.current = null;
  }

  async function pollSavedPlaylistOfflineReconcile() {
    try {
      const status =
        await stereodromeCore.getSavedPlaylistsOfflineReconcileStatus();
      if (status.running) {
        return;
      }

      stopSavedPlaylistOfflinePolling();
      if (status.last_error) {
        setError(status.last_error);
      } else {
        setError(null);
      }
      await refreshOfflineSongIds();
    } catch (e) {
      stopSavedPlaylistOfflinePolling();
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  function startSavedPlaylistOfflinePolling() {
    if (savedPlaylistOfflinePoll.current) {
      return;
    }
    void pollSavedPlaylistOfflineReconcile();
    savedPlaylistOfflinePoll.current = setInterval(
      () => void pollSavedPlaylistOfflineReconcile(),
      savedPlaylistOfflinePollIntervalMs
    );
  }

  async function reconcileSavedPlaylistsOfflineInBackground() {
    await stereodromeCore.startSavedPlaylistsOfflineReconcile();
    startSavedPlaylistOfflinePolling();
  }

  async function refreshStatus() {
    try {
      const connectivitySettings =
        await stereodromeCore.getConnectivitySettings();
      setManualOfflineEnabledState(connectivitySettings.manual_offline_enabled);
      const next = await stereodromeCore.restoreSession();
      setStatus(next);
      setError(null);
      if (next.server_url) {
        await refreshOfflineSongIds();
        if (next.connected && !connectivitySettings.manual_offline_enabled) {
          void reconcileSavedPlaylistsOfflineInBackground().catch((e) =>
            setError(e instanceof Error ? e.message : String(e))
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
  }

  async function connect(params: {
    url: string;
    username: string;
    password: string;
  }) {
    if (manualOfflineEnabled) {
      throw new Error("Offline mode is enabled");
    }

    const next = await stereodromeCore.connectServer(params);
    setStatus(next);
    setError(null);
    await reconcileSavedPlaylistsOfflineInBackground();
    await refreshOfflineSongIds();
  }

  async function updateServerSettings(params: {
    url?: string;
    username?: string;
  }) {
    if (manualOfflineEnabled) {
      throw new Error("Offline mode is enabled");
    }

    const next = await stereodromeCore.updateServerSettings(params);
    setStatus(next);
    setError(null);
    if (next.connected && !manualOfflineEnabled) {
      await reconcileSavedPlaylistsOfflineInBackground();
    }
    await refreshOfflineSongIds();
  }

  async function sync() {
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
  }

  async function syncIncremental() {
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
  }

  async function refreshLibraryAfterSync() {
    await Promise.all(
      libraryQueryKeys.map((queryKey) =>
        queryClient.invalidateQueries({ queryKey })
      )
    );
    if (status.connected && !manualOfflineEnabled) {
      await reconcileSavedPlaylistsOfflineInBackground();
    }
    await refreshOfflineSongIds();
  }

  async function refreshSyncStatusAfterForeground() {
    if (manualOfflineEnabled) {
      return;
    }

    try {
      const syncStatus = await stereodromeCore.getLibrarySyncStatus();
      queryClient.setQueryData(librarySyncStatusQueryKey, syncStatus);

      const nextCompletedSyncKey = completedSyncKey(syncStatus);
      if (
        lastCompletedSyncKey.current &&
        nextCompletedSyncKey !== lastCompletedSyncKey.current
      ) {
        await refreshLibraryAfterSync();
      }
      lastCompletedSyncKey.current = nextCompletedSyncKey;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  useEffect(() => {
    let mounted = true;
    stereodromeCore
      .initialize()
      .then(async () => {
        await refreshStatus();
        await syncLibraryBackgroundRegistration();
      })
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
      stopSavedPlaylistOfflinePolling();
    };
  }, []);

  useEffect(() => {
    return stereodromeCore.addEventListener<PlaybackSnapshot>(
      "playback-snapshot",
      (snapshot) => {
        setOfflineSongIds(new Set(snapshot.downloaded_song_ids ?? []));
        setDownloadingSongIds(new Set(snapshot.downloading_song_ids ?? []));
      }
    );
  }, []);

  useEffect(() => {
    return stereodromeCore.addEventListener<LibrarySyncStatus>(
      "sync-status-changed",
      (syncStatus) => {
        queryClient.setQueryData(librarySyncStatusQueryKey, syncStatus);
        lastCompletedSyncKey.current = completedSyncKey(syncStatus);

        const wasActive = syncWasActive.current;
        const isActive = Boolean(syncStatus.active_job);
        syncWasActive.current = isActive;

        if (wasActive && !isActive) {
          void refreshLibraryAfterSync();
        }
      }
    );
  }, [queryClient]);

  useEffect(() => {
    function handleAppStateChange(nextState: AppStateStatus) {
      if (nextState === "active") {
        void refreshSyncStatusAfterForeground();
        if (!manualOfflineEnabled) {
          void syncLibraryBackgroundRegistration();
        }
      }
    }

    const subscription = AppState.addEventListener(
      "change",
      handleAppStateChange
    );
    return () => subscription.remove();
  }, [queryClient]);

  async function setManualOfflineEnabled(enabled: boolean) {
    const settings = await stereodromeCore.setConnectivitySettings({
      manual_offline_enabled: enabled,
    });
    setManualOfflineEnabledState(settings.manual_offline_enabled);
    await refreshStatus();
    await syncLibraryBackgroundRegistration();
  }

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
      ready,
      status,
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
