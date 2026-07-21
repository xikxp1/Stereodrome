import { StatusBar } from "expo-status-bar";
import { SafeAreaProvider } from "react-native-safe-area-context";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import {
  focusManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { AppState, StyleSheet } from "react-native";
import { useEffect, useMemo } from "react";

import { IpodShell } from "@/components/IpodShell";
import { InputProvider } from "@/context/InputContext";
import { MobileSettingsProvider } from "@/context/MobileSettingsContext";
import { PlaybackProvider } from "@/context/PlaybackContext";
import { SongActionProvider } from "@/context/SongActionContext";
import { StereodromeProvider } from "@/context/StereodromeContext";
import { ViewProvider } from "@/context/ViewContext";
import "@/services/librarySyncScheduler";

export default function App() {
  useEffect(() => {
    focusManager.setFocused(AppState.currentState === "active");
    const subscription = AppState.addEventListener("change", (state) => {
      focusManager.setFocused(state === "active");
    });
    return () => {
      subscription.remove();
      focusManager.setFocused(undefined);
    };
  }, []);

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

  return (
    <GestureHandlerRootView style={styles.root}>
      <SafeAreaProvider>
        <QueryClientProvider client={queryClient}>
          <StereodromeProvider>
            <MobileSettingsProvider>
              <PlaybackProvider>
                <InputProvider>
                  <ViewProvider>
                    <SongActionProvider>
                      <IpodShell />
                    </SongActionProvider>
                  </ViewProvider>
                </InputProvider>
              </PlaybackProvider>
            </MobileSettingsProvider>
          </StereodromeProvider>
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
