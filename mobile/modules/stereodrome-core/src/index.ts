import { requireNativeModule, type EventSubscription } from "expo-modules-core";

export type StereodromeCoreNativeModule = {
  initialize?(dataDir: string): Promise<boolean>;
  dispatch?(commandJson: string): Promise<string>;
  addListener?(
    eventName: "core-event",
    listener: (payload: { event: string }) => void
  ): EventSubscription;
};

export default requireNativeModule<StereodromeCoreNativeModule>(
  "StereodromeCore"
);
