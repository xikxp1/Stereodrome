# Stereodrome Mobile

Expo React Native dev-build app for Stereodrome’s mobile interface.

## Development

Install dependencies from this directory:

```sh
vp install
```

Build the Rust native library before creating native dev builds:

```sh
vp run rust:ios
```

Android builds additionally require the Android SDK/NDK and `cargo-ndk`.
The Rust audio backend links Android AAudio, so builds target Android API 26
or newer:

```sh
cargo install cargo-ndk
vp run rust:android
```

Start Metro for an Expo development build:

```sh
vp run start
```

The app uses a local Expo module in `modules/stereodrome-core` to call the Rust
FFI crate through JSON envelopes. It is not expected to run in Expo Go.
