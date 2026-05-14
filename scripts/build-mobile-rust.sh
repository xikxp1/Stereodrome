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

  local framework_build_dir="$IOS_OUT_DIR/build"
  local device_framework="$framework_build_dir/ios-arm64/StereodromeFfi.framework"
  local simulator_framework="$framework_build_dir/ios-arm64_x86_64-simulator/StereodromeFfi.framework"
  local sim_dir="$framework_build_dir/simulator"
  mkdir -p "$sim_dir"
  local sim_universal="$sim_dir/libstereodrome_ffi.a"
  lipo -create \
    "$ROOT_DIR/target/aarch64-apple-ios-sim/release/libstereodrome_ffi.a" \
    "$ROOT_DIR/target/x86_64-apple-ios/release/libstereodrome_ffi.a" \
    -output "$sim_universal"

  create_static_framework \
    "$ROOT_DIR/target/aarch64-apple-ios/release/libstereodrome_ffi.a" \
    "$device_framework"
  create_static_framework \
    "$sim_universal" \
    "$simulator_framework"

  xcodebuild -create-xcframework \
    -framework "$device_framework" \
    -framework "$simulator_framework" \
    -output "$IOS_OUT_DIR/StereodromeFfi.xcframework"

  rm -rf "$framework_build_dir"
}

create_static_framework() {
  local library_path="$1"
  local framework_path="$2"

  rm -rf "$framework_path"
  mkdir -p "$framework_path/Headers" "$framework_path/Modules"

  cp "$library_path" "$framework_path/StereodromeFfi"
  cp "$FFI_INCLUDE_DIR/stereodrome_ffi.h" "$framework_path/Headers/stereodrome_ffi.h"

  cat > "$framework_path/Modules/module.modulemap" <<'EOF'
framework module StereodromeFfi {
  umbrella header "stereodrome_ffi.h"

  export *
  module * { export * }
}
EOF

  cat > "$framework_path/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>StereodromeFfi</string>
  <key>CFBundleIdentifier</key>
  <string>dev.xikxp1.stereodrome.ffi</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>StereodromeFfi</string>
  <key>CFBundlePackageType</key>
  <string>FMWK</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>MinimumOSVersion</key>
  <string>15.1</string>
</dict>
</plist>
EOF
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
