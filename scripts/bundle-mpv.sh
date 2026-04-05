#!/bin/bash
# bundle-mpv.sh — Bundle mpv + libs + uosc for standalone awawapp
#
# This script prepares the mpv sidecar binary, its shared libraries,
# and uosc scripts so the final app is fully self-contained.
#
# Supports: macOS (arm64/x86_64), Linux (x86_64/aarch64), Windows (x86_64)
#
# Usage:
#   ./scripts/bundle-mpv.sh
#
# Prerequisites (build machine only):
#   macOS:   brew install mpv
#   Linux:   sudo apt install mpv libmpv-dev (or equivalent)
#   Windows: download mpv zip from https://mpv.io/installation/ and add to PATH
#
# After running, the user does NOT need mpv installed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BINARIES_DIR="$PROJECT_ROOT/src-tauri/binaries"
LIB_DIR="$PROJECT_ROOT/src-tauri/lib"
MPV_CONFIG_DIR="$PROJECT_ROOT/src-tauri/mpv-config"

# uosc version to bundle
UOSC_VERSION="5.12.0"

# ── Detect platform & architecture ─────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin)
        PLATFORM="macos"
        if [ "$ARCH" = "arm64" ]; then
            TARGET_TRIPLE="aarch64-apple-darwin"
        else
            TARGET_TRIPLE="x86_64-apple-darwin"
        fi
        MPV_EXE_NAME="mpv"
        LIB_EXT="dylib"
        ;;
    Linux)
        PLATFORM="linux"
        if [ "$ARCH" = "aarch64" ]; then
            TARGET_TRIPLE="aarch64-unknown-linux-gnu"
        else
            TARGET_TRIPLE="x86_64-unknown-linux-gnu"
        fi
        MPV_EXE_NAME="mpv"
        LIB_EXT="so"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        PLATFORM="windows"
        TARGET_TRIPLE="x86_64-pc-windows-msvc"
        MPV_EXE_NAME="mpv.exe"
        LIB_EXT="dll"
        ;;
    *)
        echo "ERROR: Unsupported OS: $OS"
        exit 1
        ;;
esac

echo "=== Bundle mpv for awawapp ==="
echo "  Platform: $PLATFORM ($ARCH)"
echo "  Target:   $TARGET_TRIPLE"
echo ""

# ── Step 1: Bundle mpv binary ───────────────────────────────────────────────

echo "── Step 1: mpv binary ──"

MPV_BIN="$(which mpv || true)"
if [ -z "$MPV_BIN" ]; then
    echo "ERROR: mpv not found in PATH."
    case "$PLATFORM" in
        macos)   echo "  Install: brew install mpv" ;;
        linux)   echo "  Install: sudo apt install mpv" ;;
        windows) echo "  Download from https://mpv.io/installation/ and add to PATH" ;;
    esac
    echo "  (End users will NOT need this — the binary gets bundled.)"
    exit 1
fi

echo "  Found mpv at: $MPV_BIN"

mkdir -p "$BINARIES_DIR"

MPV_DEST="$BINARIES_DIR/mpv-${TARGET_TRIPLE}"
if [ "$PLATFORM" = "windows" ]; then
    MPV_DEST="$BINARIES_DIR/mpv-${TARGET_TRIPLE}.exe"
fi

echo "  Copying to: $MPV_DEST"
if [ -f "$MPV_DEST" ]; then
    chmod u+w "$MPV_DEST" 2>/dev/null || true
    rm -f "$MPV_DEST"
fi
cp "$MPV_BIN" "$MPV_DEST"
chmod +x "$MPV_DEST"

echo "  Done."
echo ""

# ── Step 2: Bundle shared libraries ────────────────────────────────────────

echo "── Step 2: Bundle shared libraries ──"

# Preserve libmpv-wrapper if it exists (it's committed to the repo)
WRAPPER_FILE="$LIB_DIR/libmpv-wrapper.dylib"
WRAPPER_BACKUP=""
if [ -f "$WRAPPER_FILE" ]; then
    WRAPPER_BACKUP="$(mktemp)"
    cp "$WRAPPER_FILE" "$WRAPPER_BACKUP"
fi

rm -rf "$LIB_DIR"
mkdir -p "$LIB_DIR"

if [ -n "$WRAPPER_BACKUP" ] && [ -f "$WRAPPER_BACKUP" ]; then
    cp "$WRAPPER_BACKUP" "$WRAPPER_FILE"
    rm -f "$WRAPPER_BACKUP"
fi

# ── Platform-specific library bundling ──

case "$PLATFORM" in
    macos)
        # macOS: use otool + install_name_tool
        # In the .app bundle: Contents/MacOS/mpv, Contents/Resources/lib/
        RPATH_PREFIX="@executable_path/../Resources/lib"

        collect_nonsystem_dylibs() {
            local binary="$1"
            otool -L "$binary" 2>/dev/null | tail -n +2 | awk '{print $1}' | while read -r dep; do
                case "$dep" in
                    /System/*|/usr/lib/*|@executable_path/*|@loader_path/*|@rpath/*)
                        continue ;;
                esac
                echo "$dep"
            done
        }

        bundle_macos_recursive() {
            local binary="$1"
            local deps
            deps="$(collect_nonsystem_dylibs "$binary")"
            [ -z "$deps" ] && return

            echo "$deps" | while read -r dep_path; do
                local dep_name
                dep_name="$(basename "$dep_path")"

                # Rewrite reference regardless
                install_name_tool -change "$dep_path" "$RPATH_PREFIX/$dep_name" "$binary" 2>/dev/null || true

                # Skip if already bundled
                [ -f "$LIB_DIR/$dep_name" ] && continue

                # Resolve symlinks
                local real_path="$dep_path"
                [ -L "$dep_path" ] && real_path="$(readlink -f "$dep_path")"

                if [ ! -f "$real_path" ]; then
                    echo "    WARNING: not found: $dep_path"
                    continue
                fi

                echo "    Bundling: $dep_name"
                cp "$real_path" "$LIB_DIR/$dep_name"
                chmod u+w "$LIB_DIR/$dep_name"
                install_name_tool -id "$RPATH_PREFIX/$dep_name" "$LIB_DIR/$dep_name" 2>/dev/null || true

                # Recurse
                bundle_macos_recursive "$LIB_DIR/$dep_name"
            done
        }

        echo "  Scanning mpv dependencies (macOS)..."
        bundle_macos_recursive "$MPV_DEST"

        if [ -f "$WRAPPER_FILE" ]; then
            chmod u+w "$WRAPPER_FILE"
            echo "  Scanning libmpv-wrapper.dylib dependencies..."
            bundle_macos_recursive "$WRAPPER_FILE"
        fi
        ;;

    linux)
        # Linux: use ldd + patchelf
        # In the AppImage/deb: binary and libs are colocated or use $ORIGIN rpath
        #
        # Tauri on Linux puts externalBin next to the main binary and
        # resources under <prefix>/lib/<app>/  — we use $ORIGIN/../lib/<app>/ for libs.
        # Simpler approach: put libs next to the binary and use $ORIGIN.

        if ! command -v patchelf &>/dev/null; then
            echo "  WARNING: patchelf not found. Install it for fully portable builds:"
            echo "    sudo apt install patchelf"
            echo "  Skipping library bundling — mpv will use system libs at runtime."
        else
            collect_nonsystem_so() {
                local binary="$1"
                ldd "$binary" 2>/dev/null | grep "=> /" | awk '{print $3}' | while read -r lib; do
                    # Skip system/core libs that are guaranteed on any Linux
                    case "$lib" in
                        /usr/lib/x86_64-linux-gnu/ld-linux*|/lib/x86_64-linux-gnu/ld-linux*)
                            continue ;;
                    esac
                    local libname
                    libname="$(basename "$lib")"
                    case "$libname" in
                        libc.so*|libm.so*|libdl.so*|librt.so*|libpthread.so*|libstdc++.so*|libgcc_s.so*|ld-linux*)
                            continue ;;
                        libX11.so*|libxcb.so*|libdrm.so*|libGL.so*|libEGL.so*|libvulkan.so*|libwayland*.so*)
                            # GPU/display libs must come from the user's system
                            continue ;;
                    esac
                    echo "$lib"
                done
            }

            bundle_linux_recursive() {
                local binary="$1"
                local deps
                deps="$(collect_nonsystem_so "$binary")"
                [ -z "$deps" ] && return

                echo "$deps" | while read -r dep_path; do
                    local dep_name
                    dep_name="$(basename "$dep_path")"

                    [ -f "$LIB_DIR/$dep_name" ] && continue

                    local real_path="$dep_path"
                    [ -L "$dep_path" ] && real_path="$(readlink -f "$dep_path")"

                    if [ ! -f "$real_path" ]; then
                        echo "    WARNING: not found: $dep_path"
                        continue
                    fi

                    echo "    Bundling: $dep_name"
                    cp "$real_path" "$LIB_DIR/$dep_name"
                    chmod u+w "$LIB_DIR/$dep_name"

                    bundle_linux_recursive "$LIB_DIR/$dep_name"
                done
            }

            echo "  Scanning mpv dependencies (Linux)..."
            bundle_linux_recursive "$MPV_DEST"

            # Set RPATH on mpv binary to find bundled libs
            # Tauri puts resources in ../lib/<appname>/ relative to the binary
            patchelf --set-rpath '$ORIGIN/../lib/awawapp' "$MPV_DEST" 2>/dev/null || \
                patchelf --set-rpath '$ORIGIN' "$MPV_DEST" 2>/dev/null || \
                echo "    WARNING: failed to set rpath on mpv"

            # Also set rpath on each bundled lib so they can find each other
            find "$LIB_DIR" -name "*.so*" | while read -r lib; do
                patchelf --set-rpath '$ORIGIN' "$lib" 2>/dev/null || true
            done
        fi
        ;;

    windows)
        # Windows: mpv official builds are self-contained zips.
        # Just need to copy DLLs that are next to mpv.exe.
        MPV_DIR="$(dirname "$MPV_BIN")"

        echo "  Copying DLLs from mpv directory: $MPV_DIR"
        DLL_COUNT=0
        for dll in "$MPV_DIR"/*.dll; do
            [ -f "$dll" ] || continue
            dll_name="$(basename "$dll")"
            echo "    Bundling: $dll_name"
            cp "$dll" "$LIB_DIR/$dll_name"
            DLL_COUNT=$((DLL_COUNT + 1))
        done

        if [ "$DLL_COUNT" -eq 0 ]; then
            echo "  WARNING: No DLLs found next to mpv.exe."
            echo "  If using a system-installed mpv, copy its DLLs manually to src-tauri/lib/"
        fi
        ;;
esac

LIB_COUNT=$(find "$LIB_DIR" -name "*.$LIB_EXT" -o -name "*.so.*" 2>/dev/null | wc -l | tr -d ' ')
echo "  Bundled $LIB_COUNT libraries to: $LIB_DIR"
echo ""

# ── Step 3: Code sign (macOS only) ────────────────────────────────────────

if [ "$PLATFORM" = "macos" ]; then
    echo "── Step 3: Code signing (macOS) ──"

    find "$LIB_DIR" -name "*.dylib" | while read -r lib; do
        codesign --force --sign - "$lib" 2>/dev/null || echo "  Warning: failed to sign $(basename "$lib")"
    done
    echo "  Signed dylibs."

    codesign --force --sign - "$MPV_DEST" 2>/dev/null || echo "  Warning: failed to sign mpv"
    echo "  Signed mpv."
    echo "  Done."
    echo ""
fi

# ── Step 4: Bundle uosc scripts ─────────────────────────────────────────────

echo "── Step 4: uosc v${UOSC_VERSION} ──"

UOSC_DIR="$MPV_CONFIG_DIR/scripts/uosc"
UOSC_ZIP_URL="https://github.com/tomasklaen/uosc/releases/download/${UOSC_VERSION}/uosc.zip"
TMP_DIR="$(mktemp -d)"

if [ -d "$UOSC_DIR" ]; then
    echo "  Removing old uosc..."
    rm -rf "$UOSC_DIR"
fi

echo "  Downloading uosc v${UOSC_VERSION}..."
if curl -fsSL "$UOSC_ZIP_URL" -o "$TMP_DIR/uosc.zip"; then
    echo "  Extracting..."
    unzip -q "$TMP_DIR/uosc.zip" -d "$TMP_DIR/uosc-extracted"

    if [ -d "$TMP_DIR/uosc-extracted/scripts/uosc" ]; then
        mkdir -p "$MPV_CONFIG_DIR/scripts"
        cp -R "$TMP_DIR/uosc-extracted/scripts/uosc" "$UOSC_DIR"
        echo "  Installed uosc scripts to: $UOSC_DIR"
    elif [ -d "$TMP_DIR/uosc-extracted/uosc" ]; then
        mkdir -p "$MPV_CONFIG_DIR/scripts"
        cp -R "$TMP_DIR/uosc-extracted/uosc" "$UOSC_DIR"
        echo "  Installed uosc scripts to: $UOSC_DIR"
    else
        echo "  Warning: unexpected uosc zip structure. Contents:"
        ls -la "$TMP_DIR/uosc-extracted/"
        mkdir -p "$UOSC_DIR"
        find "$TMP_DIR/uosc-extracted" -name "*.lua" -exec cp {} "$UOSC_DIR/" \;
        echo "  Copied lua files to: $UOSC_DIR"
    fi

    echo "  Keeping custom awawapp uosc.conf (not overwriting with default)"
else
    echo "  Warning: Failed to download uosc. Trying local fallback..."
    LOCAL_UOSC="$HOME/.config/mpv/scripts/uosc"
    if [ -d "$LOCAL_UOSC" ]; then
        mkdir -p "$MPV_CONFIG_DIR/scripts"
        cp -R "$LOCAL_UOSC" "$UOSC_DIR"
        echo "  Copied local uosc from: $LOCAL_UOSC"
    else
        echo "  ERROR: Could not download or find uosc."
        echo "  The app will work without uosc but with basic mpv controls."
    fi
fi

rm -rf "$TMP_DIR"

echo "  Done."
echo ""

# ── Step 5: Verify bundle ──────────────────────────────────────────────────

echo "── Verification ──"

echo "  mpv binary:"
if [ -f "$MPV_DEST" ]; then
    echo "    OK: $MPV_DEST ($(du -h "$MPV_DEST" | cut -f1))"
else
    echo "    MISSING: $MPV_DEST"
fi

echo "  Bundled libraries:"
if [ -d "$LIB_DIR" ]; then
    LIB_SIZE=$(du -sh "$LIB_DIR" | cut -f1)
    echo "    OK: $LIB_COUNT libs ($LIB_SIZE total)"
else
    echo "    MISSING: $LIB_DIR"
fi

# Platform-specific portability check
case "$PLATFORM" in
    macos)
        echo "  Portability check:"
        REMAINING=$(otool -L "$MPV_DEST" 2>/dev/null | tail -n +2 | awk '{print $1}' | \
            grep -v '/System/' | grep -v '/usr/lib/' | grep -v '@executable_path' || true)
        if [ -z "$REMAINING" ]; then
            echo "    OK: All deps are system or @executable_path (fully portable)"
        else
            echo "    WARNING: Unbundled deps remain:"
            echo "$REMAINING" | sed 's/^/      /'
        fi
        ;;
    linux)
        if command -v patchelf &>/dev/null; then
            echo "  RPATH of mpv binary:"
            RPATH=$(patchelf --print-rpath "$MPV_DEST" 2>/dev/null || echo "(unknown)")
            echo "    $RPATH"
        fi
        ;;
esac

echo "  uosc scripts:"
if [ -d "$UOSC_DIR" ]; then
    UOSC_FILES=$(find "$UOSC_DIR" -name "*.lua" | wc -l | tr -d ' ')
    echo "    OK: $UOSC_DIR ($UOSC_FILES lua files)"
else
    echo "    MISSING: $UOSC_DIR"
fi

echo "  mpv config:"
[ -f "$MPV_CONFIG_DIR/mpv.conf" ] && echo "    OK" || echo "    MISSING"

echo "  uosc config:"
[ -f "$MPV_CONFIG_DIR/script-opts/uosc.conf" ] && echo "    OK" || echo "    MISSING"

echo ""
echo "=== Bundle complete ($PLATFORM $ARCH) ==="
echo ""
echo "Next: npm run tauri:build"
