import { describe, expect, it } from "vite-plus/test";

import playbackSnapshotFixture from "../../modules/stereodrome-core/fixtures/playback-snapshot.json";

import type { PlaybackSnapshot } from "../types/music";

describe("mobile core protocol fixtures", () => {
  it("matches the TypeScript playback snapshot contract", () => {
    const typedFixture: PlaybackSnapshot = {
      ...playbackSnapshotFixture,
      state: "playing",
      output_state: "ready",
      queue: {
        ...playbackSnapshotFixture.queue,
        repeat_mode: "All",
      },
    };

    expect(typedFixture.seq).toBe(42);
    expect(typedFixture.song?.id).toBe("song-b");
    expect(typedFixture.queue.current_index).toBe(1);
    expect(typedFixture.can_seek).toBe(true);
  });
});
