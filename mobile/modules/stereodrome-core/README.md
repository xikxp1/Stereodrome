# Stereodrome Core Native Module

This local Expo module is the mobile boundary for `crates/stereodrome-ffi`.

Current shape:

- TypeScript calls `StereodromeCore.dispatch(commandJson)` with a versioned
  `CoreCommandRequest`.
- The Rust FFI crate exposes only the `stereodrome_runtime_*` lifecycle,
  dispatch, snapshot, and event callback functions.
- Results and events use the types generated from
  `crates/stereodrome-core/src/protocol.rs`.
- The event callback forwards one unwrapped, ordered `CoreEvent` stream.

The Swift/Kotlin module files are intentionally thin. They keep one native
runtime handle alive after `initialize(dataDir)`, forward typed dispatches, map
`snapshot.playback` to platform media APIs, and forward runtime events to JS
when it is awake. The wire contract is documented in
`docs/MOBILE_RUNTIME_PROTOCOL.md`.

Before creating a dev build, generate the Rust native artifacts:

```sh
vp run rust:ios
vp run rust:android
```

The iOS script writes `ios/rust-libs/StereodromeFfi.xcframework`.
The Android script writes ABI-specific static libraries under
`android/rust-libs/<abi>/libstereodrome_ffi.a`. Android builds target API 26
or newer because the Rust audio backend links Android AAudio.
