import { requireNativeModule, type EventSubscription } from "expo-modules-core";

export type StereodromeCoreNativeModule = {
  initialize?(dataDir: string): Promise<boolean>;
  dispatch?(commandJson: string): Promise<string>;
  startResourceDiagnostics?(): Promise<string>;
  stopResourceDiagnostics?(): Promise<string>;
  getResourceDiagnosticsStatus?(): Promise<string>;
  exportResourceDiagnostics?(destinationPath: string): Promise<boolean>;
  clearResourceDiagnostics?(): Promise<boolean>;
  addListener?(
    eventName: "core-event",
    listener: (payload: { event: string }) => void
  ): EventSubscription;
};

export default requireNativeModule<StereodromeCoreNativeModule>(
  "StereodromeCore"
);
