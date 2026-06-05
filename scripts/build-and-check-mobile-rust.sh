#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_SCRIPT="$ROOT_DIR/scripts/build-mobile-rust.sh"
IOS_OUT_DIR="$ROOT_DIR/mobile/modules/stereodrome-core/ios/rust-libs"
ANDROID_OUT_DIR="$ROOT_DIR/mobile/modules/stereodrome-core/android/rust-libs"

usage() {
  cat <<'USAGE'
Usage: scripts/build-and-check-mobile-rust.sh [ios|android|all]

Checks the shared mobile Rust crates, builds stereodrome-ffi for the selected
mobile platform(s), and verifies the expected native artifacts were produced.

Requirements:
  iOS: Xcode command line tools and rustup iOS targets.
  Android: Android NDK plus cargo-ndk (`cargo install cargo-ndk`).
USAGE
}

run_checks() {
  cargo fmt --check
  cargo check -p stereodrome-core -p stereodrome-ffi
  cargo test -p stereodrome-core -p stereodrome-ffi
  cargo clippy -p stereodrome-core -p stereodrome-ffi -- -D warnings
}

verify_ios() {
  local framework="$IOS_OUT_DIR/StereodromeFfi.xcframework"
  local header="$framework/ios-arm64/StereodromeFfi.framework/Headers/stereodrome_ffi.h"
  local binary="$framework/ios-arm64/StereodromeFfi.framework/StereodromeFfi"
  local sim_binary="$framework/ios-arm64_x86_64-simulator/StereodromeFfi.framework/StereodromeFfi"

  [[ -d "$framework" ]] || missing "$framework"
  [[ -f "$header" ]] || missing "$header"
  [[ -f "$binary" ]] || missing "$binary"
  [[ -f "$sim_binary" ]] || missing "$sim_binary"
}

verify_android() {
  local abi
  for abi in arm64-v8a armeabi-v7a x86 x86_64; do
    [[ -f "$ANDROID_OUT_DIR/$abi/libstereodrome_ffi.a" ]] || \
      missing "$ANDROID_OUT_DIR/$abi/libstereodrome_ffi.a"
  done
}

missing() {
  echo "Missing expected mobile Rust artifact: $1" >&2
  exit 1
}

target="${1:-all}"
case "$target" in
  ios|android|all)
    ;;
  -h|--help|help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 1
    ;;
esac

cd "$ROOT_DIR"
run_checks
"$BUILD_SCRIPT" "$target"

case "$target" in
  ios)
    verify_ios
    ;;
  android)
    verify_android
    ;;
  all)
    verify_ios
    verify_android
    ;;
esac

echo "Mobile Rust build and checks passed for: $target"
