#!/usr/bin/env bash
set -euo pipefail

PLATFORM="all"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --platform)
            PLATFORM="${2:?missing platform}"
            shift 2
            ;;
        --platform=*)
            PLATFORM="${1#*=}"
            shift
            ;;
        -h|--help)
            cat <<'EOF'
Usage: ./build-all.sh [--platform all|linux|windows|macos]

Builds and packages the plugin for the requested platform(s). "all" builds every
platform available on this machine; macOS is skipped with setup instructions when
its toolchain is unavailable.
EOF
            exit 0
            ;;
        *)
            echo "unknown option: $1" >&2
            exit 2
            ;;
    esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

DIST="$ROOT/dist"
CRATES=(instrument audio-fx midi-fx)

make_zip() {
    local archive="$1"
    local dir="$2"
    if command -v zip >/dev/null 2>&1; then
        (cd "$dir" && zip -r -FS "$archive" . >/dev/null)
    else
        python3 - "$dir" "$archive" <<'PY'
import os
import sys
import zipfile

root, archive = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
    for dirpath, _, filenames in os.walk(root):
        for filename in filenames:
            path = os.path.join(dirpath, filename)
            output.write(path, os.path.relpath(path, root))
PY
    fi
}

crate_lib() {
    case "$1" in
        instrument) echo "lua_plugin_instrument" ;;
        audio-fx) echo "lua_plugin_audio_fx" ;;
        midi-fx) echo "lua_plugin_midi_fx" ;;
        *) echo "unknown crate: $1" >&2; return 1 ;;
    esac
}

plugin_slug() {
    case "$1" in
        instrument) echo "lua-plugin-instrument" ;;
        audio-fx) echo "lua-plugin-audio-fx" ;;
        midi-fx) echo "lua-plugin-midi-fx" ;;
        *) echo "unknown crate: $1" >&2; return 1 ;;
    esac
}

build_target() {
    local target="$1"
    echo "==> Building $target"
    cargo build --workspace --release --target "$target"
}

package_linux() {
    local target="x86_64-unknown-linux-gnu"
    local out="$DIST/linux"
    rm -rf "$out"
    mkdir -p "$out/raw" "$out/clap" "$out/vst3"
    for crate in "${CRATES[@]}"; do
        local lib out_name
        lib="$(crate_lib "$crate")"
        out_name="$(plugin_slug "$crate")"
        local src="$ROOT/target/$target/release/lib${lib}.so"
        cp "$src" "$out/raw/${out_name}.so"
        cp "$src" "$out/clap/${out_name}.clap"
        mkdir -p "$out/vst3/${out_name}.vst3/Contents/x86_64-linux"
        cp "$src" "$out/vst3/${out_name}.vst3/Contents/x86_64-linux/${out_name}.vst3"
    done
    make_zip "$DIST/lua-plugin-host-linux-x86_64.zip" "$out"
    echo "==> Packaged Linux artifacts in $out"
}

package_windows() {
    local target="x86_64-pc-windows-gnu"
    local out="$DIST/windows"
    rm -rf "$out"
    mkdir -p "$out/raw" "$out/clap" "$out/vst3"
    for crate in "${CRATES[@]}"; do
        local lib out_name
        lib="$(crate_lib "$crate")"
        out_name="$(plugin_slug "$crate")"
        local src="$ROOT/target/$target/release/${lib}.dll"
        cp "$src" "$out/raw/${out_name}.dll"
        cp "$src" "$out/clap/${out_name}.clap"
        mkdir -p "$out/vst3/${out_name}.vst3/Contents/x86_64-win"
        cp "$src" "$out/vst3/${out_name}.vst3/Contents/x86_64-win/${out_name}.vst3"
    done
    make_zip "$DIST/lua-plugin-host-windows-x86_64.zip" "$out"
    echo "==> Packaged Windows artifacts in $out"
}

write_macos_bundle_metadata() {
    local bundle="$1"
    local executable="$2"
    local identifier="$3"
    local name="$4"
    mkdir -p "$bundle/Contents/MacOS"
    cat >"$bundle/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>$executable</string>
    <key>CFBundleIdentifier</key>
    <string>$identifier</string>
    <key>CFBundleName</key>
    <string>$name</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF
    printf 'BNDL????' >"$bundle/Contents/PkgInfo"
}

package_macos() {
    local out="$DIST/macos"
    rm -rf "$out"
    mkdir -p "$out/raw" "$out/clap" "$out/vst3"
    for crate in "${CRATES[@]}"; do
        local lib out_name identifier display_name
        lib="$(crate_lib "$crate")"
        out_name="$(plugin_slug "$crate")"
        identifier="com.lua-plugin-host.${crate}"
        display_name="Lua Plugin"
        case "$crate" in
            audio-fx) display_name="Lua Audio FX" ;;
            midi-fx) display_name="Lua MIDI FX" ;;
            instrument) display_name="Lua Instrument" ;;
        esac

        local x86="$ROOT/target/x86_64-apple-darwin/release/lib${lib}.dylib"
        local arm="$ROOT/target/aarch64-apple-darwin/release/lib${lib}.dylib"
        local universal="$out/raw/${out_name}.dylib"
        lipo -create "$x86" "$arm" -output "$universal"

        cp "$universal" "$out/clap/${out_name}.clap"
        write_macos_bundle_metadata "$out/clap/${out_name}.clap" "$out_name" "$identifier" "$display_name"

        local vst3_bundle="$out/vst3/${out_name}.vst3"
        write_macos_bundle_metadata "$vst3_bundle" "$out_name" "$identifier" "$display_name"
        cp "$universal" "$vst3_bundle/Contents/MacOS/${out_name}.vst3"
    done
    make_zip "$DIST/lua-plugin-host-macos-universal.zip" "$out"
    echo "==> Packaged macOS artifacts in $out"
}

if [[ "$PLATFORM" == "all" || "$PLATFORM" == "linux" ]]; then
    echo "==> Building Linux"
    build_target "x86_64-unknown-linux-gnu"
    package_linux
fi

if [[ "$PLATFORM" == "all" || "$PLATFORM" == "windows" ]]; then
    echo "==> Building Windows"
    build_target "x86_64-pc-windows-gnu"
    package_windows
fi

if [[ "$PLATFORM" == "all" || "$PLATFORM" == "macos" ]]; then
    echo "==> Building macOS (universal)"
    if rustup target list --installed | grep -qx 'x86_64-apple-darwin' \
        && rustup target list --installed | grep -qx 'aarch64-apple-darwin' \
        && command -v lipo >/dev/null 2>&1; then
    if build_target "x86_64-apple-darwin" && build_target "aarch64-apple-darwin"; then
        package_macos
    else
        echo "==> macOS toolchain is installed but the build failed; skipping macOS packaging" >&2
    fi
else
    cat >&2 <<'EOF'
==> Skipping macOS: install the macOS Rust targets and a macOS linker, then rerun.
    Example with osxcross:
      rustup target add x86_64-apple-darwin aarch64-apple-darwin
      # configure CC_x86_64_apple_darwin / CC_aarch64_apple_darwin and the
      # macOS SDK, then rerun ./build-all.sh
EOF
    fi
fi

echo "==> Done"