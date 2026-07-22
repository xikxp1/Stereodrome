# Stereodrome Core Native Module

This local Expo module is the mobile boundary for `crates/stereodrome-ffi`.

Current shape:

- TypeScript calls `StereodromeCore.call(method, payloadJson)`.
- The Rust FFI crate exposes `stereodrome_core_call(core, method, payload)`.
- Payloads and return values are JSON envelopes:
  - success: `{ "ok": true, "value": ... }`
  - failure: `{ "ok": false, "error": "..." }`

The Swift/Kotlin module files are intentionally thin. They keep one native
`StereodromeCore` handle alive after `initialize(dataDir)` and forward every
`call` invocation to `stereodrome_core_call`.

Phase 1 also exports the versioned `stereodrome_runtime_*` C ABI documented in
`docs/MOBILE_RUNTIME_PROTOCOL.md`. Existing native calls remain compatible while
the TypeScript/native clients migrate to typed commands in later phases.

Phase 2 exposes `dispatch(commandJson)` through the Swift and Kotlin Expo
adapters for lifecycle/network inputs. Runtime snapshots are projected back onto
the existing sync and saved-playlist events during the compatibility period.

Before creating a dev build, generate the Rust native artifacts:

```sh
vp run rust:ios
vp run rust:android
```

The iOS script writes `ios/rust-libs/StereodromeFfi.xcframework`.
The Android script writes ABI-specific static libraries under
`android/rust-libs/<abi>/libstereodrome_ffi.a`. Android builds target API 26
or newer because the Rust audio backend links Android AAudio.
