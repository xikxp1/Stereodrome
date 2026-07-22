import { requireNativeModule, type EventSubscription } from "expo-modules-core";

export type NativeEnvelope<T> =
  | { ok: true; value: T }
  | { ok: false; error: string };

export type StereodromeCoreNativeModule = {
  initialize?(dataDir: string): Promise<boolean>;
  call?(method: string, payload: string): Promise<string>;
  dispatch?(commandJson: string): Promise<string>;
  getConnectionStatus(): Promise<string>;
  getStreamUri(songId: string): Promise<string>;
  addListener?(
    eventName: "playback-snapshot",
    listener: (payload: { snapshot: string }) => void
  ): EventSubscription;
  addListener?(
    eventName: "core-event",
    listener: (payload: { event: string }) => void
  ): EventSubscription;
};

export default requireNativeModule<StereodromeCoreNativeModule>(
  "StereodromeCore"
);
