import { StatusBar } from "expo-status-bar";
import { SafeAreaProvider } from "react-native-safe-area-context";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StyleSheet } from "react-native";
import { useMemo } from "react";

import { IpodShell } from "@/components/IpodShell";
import { InputProvider } from "@/context/InputContext";
import { MobileSettingsProvider } from "@/context/MobileSettingsContext";
import { PlaybackProvider } from "@/context/PlaybackContext";
import { StereodromeProvider } from "@/context/StereodromeContext";
import { ViewProvider } from "@/context/ViewContext";

export default function App() {
  const queryClient = useMemo(() => new QueryClient(), []);

  return (
    <GestureHandlerRootView style={styles.root}>
      <SafeAreaProvider>
        <QueryClientProvider client={queryClient}>
          <StereodromeProvider>
            <MobileSettingsProvider>
              <PlaybackProvider>
                <InputProvider>
                  <ViewProvider>
                    <IpodShell />
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
