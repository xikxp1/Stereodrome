#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FFI_INCLUDE_DIR="$ROOT_DIR/crates/stereodrome-ffi/include"
IOS_OUT_DIR="$ROOT_DIR/mobile/modules/stereodrome-core/ios/rust-libs"
ANDROID_OUT_DIR="$ROOT_DIR/mobile/modules/stereodrome-core/android/rust-libs"

usage() {
  cat <<'USAGE'
Usage: scripts/build-mobile-rust.sh [ios|android|all]

Builds stereodrome-ffi for mobile native modules.

Requirements:
  iOS: Xcode command line tools and rustup iOS targets.
  Android: Android NDK plus cargo-ndk (`cargo install cargo-ndk`).
USAGE
}

build_ios() {
  command -v xcodebuild >/dev/null || {
    echo "xcodebuild is required for iOS XCFramework output" >&2
    exit 1
  }

  rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios

  cargo build --release -p stereodrome-ffi --target aarch64-apple-ios
  cargo build --release -p stereodrome-ffi --target aarch64-apple-ios-sim
  cargo build --release -p stereodrome-ffi --target x86_64-apple-ios

  rm -rf "$IOS_OUT_DIR"
  mkdir -p "$IOS_OUT_DIR"

  local sim_dir="$IOS_OUT_DIR/simulator"
  mkdir -p "$sim_dir"
  local sim_universal="$sim_dir/libstereodrome_ffi.a"
  lipo -create \
    "$ROOT_DIR/target/aarch64-apple-ios-sim/release/libstereodrome_ffi.a" \
    "$ROOT_DIR/target/x86_64-apple-ios/release/libstereodrome_ffi.a" \
    -output "$sim_universal"

  xcodebuild -create-xcframework \
    -library "$ROOT_DIR/target/aarch64-apple-ios/release/libstereodrome_ffi.a" \
    -headers "$FFI_INCLUDE_DIR" \
    -library "$sim_universal" \
    -headers "$FFI_INCLUDE_DIR" \
    -output "$IOS_OUT_DIR/StereodromeFfi.xcframework"
}

build_android() {
  command -v cargo-ndk >/dev/null || {
    echo "cargo-ndk is required. Install it with: cargo install cargo-ndk" >&2
    exit 1
  }

  rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android

  rm -rf "$ANDROID_OUT_DIR"
  mkdir -p "$ANDROID_OUT_DIR"

  cargo ndk \
    -t arm64-v8a \
    -t armeabi-v7a \
    -t x86 \
    -t x86_64 \
    -o "$ANDROID_OUT_DIR" \
    build --release -p stereodrome-ffi

  local abi
  local target
  for abi_target in \
    "arm64-v8a:aarch64-linux-android" \
    "armeabi-v7a:armv7-linux-androideabi" \
    "x86:i686-linux-android" \
    "x86_64:x86_64-linux-android"
  do
    abi="${abi_target%%:*}"
    target="${abi_target#*:}"
    mkdir -p "$ANDROID_OUT_DIR/$abi"
    cp \
      "$ROOT_DIR/target/$target/release/libstereodrome_ffi.a" \
      "$ANDROID_OUT_DIR/$abi/libstereodrome_ffi.a"
  done
}

case "${1:-all}" in
  ios)
    build_ios
    ;;
  android)
    build_android
    ;;
  all)
    build_ios
    build_android
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    usage
    exit 1
    ;;
esac
