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

Before creating a dev build, generate the Rust native artifacts:

```sh
vp run rust:ios
vp run rust:android
```

The iOS script writes `ios/rust-libs/StereodromeFfi.xcframework`.
The Android script writes ABI-specific static libraries under
`android/rust-libs/<abi>/libstereodrome_ffi.a`. Android builds target API 26
or newer because the Rust audio backend links Android AAudio.
