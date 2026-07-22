import { StatusBar } from "expo-status-bar";
import { SafeAreaProvider } from "react-native-safe-area-context";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import {
  focusManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import * as Network from "expo-network";
import { AppState, StyleSheet } from "react-native";
import { useEffect, useMemo } from "react";

import { IpodShell } from "@/components/IpodShell";
import { coreActions, coreStore } from "@/core/store";
import { InputProvider } from "@/context/InputContext";
import { MobileSettingsProvider } from "@/context/MobileSettingsContext";
import { SongActionProvider } from "@/context/SongActionContext";
import { ViewProvider } from "@/context/ViewContext";
import "@/services/librarySyncScheduler";
import { syncLibraryBackgroundRegistration } from "@/services/librarySyncScheduler";

function networkAvailable(state: Network.NetworkState): boolean {
  return state.isConnected !== false && state.isInternetReachable !== false;
}

function ignoreFailure(promise: Promise<unknown>): void {
  void promise.catch(() => undefined);
}

export default function App() {
  const queryClient = useMemo(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            gcTime: 10 * 60 * 1000,
            refetchOnMount: false,
            refetchOnReconnect: false,
            refetchOnWindowFocus: false,
            staleTime: 60 * 1000,
          },
        },
      }),
    []
  );

  useEffect(() => {
    let lastLibraryRevision: number | null = null;
    const unsubscribeStore = coreStore.subscribe(() => {
      const revision = coreStore.getState().snapshot?.library_revision ?? null;
      if (
        revision !== null &&
        lastLibraryRevision !== null &&
        revision > lastLibraryRevision
      ) {
        ignoreFailure(queryClient.invalidateQueries({ refetchType: "active" }));
      }
      lastLibraryRevision = revision;
    });

    focusManager.setFocused(AppState.currentState === "active");
    const appStateSubscription = AppState.addEventListener(
      "change",
      (nextState) => {
        const active = nextState === "active";
        focusManager.setFocused(active);
        ignoreFailure(
          coreActions.reportLifecycle(active ? "foreground" : "background")
        );
        if (active) {
          ignoreFailure(coreActions.reconcile());
          ignoreFailure(
            Network.getNetworkStateAsync().then((state) =>
              coreActions.reportNetwork(networkAvailable(state))
            )
          );
        }
      }
    );
    const networkSubscription = Network.addNetworkStateListener((state) => {
      ignoreFailure(coreActions.reportNetwork(networkAvailable(state)));
    });

    ignoreFailure(
      coreStore.initialize().then(async () => {
        await coreActions.reportLifecycle(
          AppState.currentState === "active" ? "foreground" : "background"
        );
        const state = await Network.getNetworkStateAsync();
        await coreActions.reportNetwork(networkAvailable(state));
        await syncLibraryBackgroundRegistration();
      })
    );

    return () => {
      unsubscribeStore();
      appStateSubscription.remove();
      networkSubscription.remove();
      focusManager.setFocused(undefined);
    };
  }, [queryClient]);

  return (
    <GestureHandlerRootView style={styles.root}>
      <SafeAreaProvider>
        <QueryClientProvider client={queryClient}>
          <MobileSettingsProvider>
            <InputProvider>
              <ViewProvider>
                <SongActionProvider>
                  <IpodShell />
                </SongActionProvider>
              </ViewProvider>
            </InputProvider>
          </MobileSettingsProvider>
        </QueryClientProvider>
        <StatusBar style="dark" />
      </SafeAreaProvider>
    </GestureHandlerRootView>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    backgroundColor: "#d8d9d4",
  },
});
