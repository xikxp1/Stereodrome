import { requireNativeModule, type EventSubscription } from "expo-modules-core";

export type NativeEnvelope<T> =
  | { ok: true; value: T }
  | { ok: false; error: string };

export type StereodromeCoreNativeModule = {
  initialize(dataDir: string): Promise<boolean>;
  call(method: string, payload: string): Promise<string>;
  getConnectionStatus(): Promise<string>;
  getStreamUri(songId: string): Promise<string>;
  setNowPlayingInfo?(payload: NativeNowPlayingInfo): Promise<void>;
  updateNowPlayingProgress?(payload: NativeNowPlayingProgress): Promise<void>;
  clearNowPlayingInfo?(): Promise<void>;
  addListener?(
    eventName: "native-playback-invalidated",
    listener: () => void
  ): EventSubscription;
};

export type NativeNowPlayingInfo = {
  song_id: string;
  title: string;
  artist: string | null;
  album: string | null;
  duration_seconds: number;
  position_seconds: number;
  is_playing: boolean;
  artwork_uri: string | null;
  queue_index: number | null;
  queue_count: number;
  can_next: boolean;
  can_play: boolean;
  can_previous: boolean;
  can_seek: boolean;
};

export type NativeNowPlayingProgress = {
  song_id: string | null;
  duration_seconds: number;
  position_seconds: number;
  is_playing: boolean;
};

export default requireNativeModule<StereodromeCoreNativeModule>(
  "StereodromeCore"
);
