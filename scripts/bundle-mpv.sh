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

MPV_BIN="$(which mpv 2>/dev/null || which mpv.exe 2>/dev/null || true)"

# On Windows in CI, auto-download mpv if not found
if [ -z "$MPV_BIN" ] && [ "$PLATFORM" = "windows" ]; then
    echo "  mpv not in PATH, downloading latest build..."
    MPV_URL=$(curl -s https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases/latest \
        | jq -r '.assets[] | select(.name | test("^mpv-x86_64-[0-9].*\\.7z$")) | .browser_download_url')
    if [ -n "$MPV_URL" ] && [ "$MPV_URL" != "null" ]; then
        TMP_MPV="$(mktemp -d)"
        echo "  Downloading from: $MPV_URL"
        curl -fsSL "$MPV_URL" -o "$TMP_MPV/mpv.7z"
        7z x "$TMP_MPV/mpv.7z" -o"$TMP_MPV/extracted" -y > /dev/null
        MPV_BIN="$(find "$TMP_MPV/extracted" -name 'mpv.exe' -type f | head -1)"
        if [ -n "$MPV_BIN" ]; then
            echo "  Downloaded mpv to: $MPV_BIN"
            # Also note the directory for DLL bundling later
            MPV_WIN_DIR="$(dirname "$MPV_BIN")"
        fi
    fi
fi

if [ -z "$MPV_BIN" ]; then
    echo "ERROR: mpv not found."
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
        # Linux: mpv has deep dependency trees (glib, cairo, icu, ffmpeg, etc.)
        # that are tightly coupled to the system. Bundling them all causes
        # version conflicts and bloats the package (~100MB+ of .so files).
        #
        # Instead, we rely on package-level dependencies:
        #   - .deb: Depends: mpv (declared in tauri.conf.json)
        #   - .rpm: Requires: mpv
        #   - AppImage: system mpv must be installed
        #
        # The mpv binary we bundle is just the one from the build system;
        # at runtime it will use the system's shared libraries.
        echo "  Linux: skipping library bundling (using system package dependencies)"
        echo "  The .deb/.rpm will declare 'mpv' as a package dependency."
        echo "  Users install mpv via: sudo apt install mpv (or equivalent)"
        ;;

    windows)
        # Windows NSIS: Tauri installs everything (main exe, sidecars, resources)
        # in the same directory (C:\Program Files\<app>\).
        # DLLs placed in resources/ will be found by mpv.exe automatically.
        MPV_DIR="${MPV_WIN_DIR:-$(dirname "$MPV_BIN")}"

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
            echo "  The app may require mpv installed on the user's system."
        else
            echo "  Bundled $DLL_COUNT DLLs"
        fi
        ;;
esac

LIB_COUNT=$(find "$LIB_DIR" -name "*.$LIB_EXT" -o -name "*.so.*" 2>/dev/null | wc -l | tr -d ' ')
echo "  Bundled $LIB_COUNT libraries to: $LIB_DIR"
echo ""

# ── Step 3: Code sign (macOS only) ────────────────────────────────────────

if [ "$PLATFORM" = "macos" ]; then
    echo "── Step 3: Code signing (macOS) ──"

    # Sign all dylibs
    find "$LIB_DIR" -name "*.dylib" | while read -r lib; do
        codesign --force --sign - "$lib" 2>/dev/null || echo "  Warning: failed to sign $(basename "$lib")"
    done
    echo "  Signed dylibs."

    # Sign any frameworks/binaries without extension (e.g., Python)
    find "$LIB_DIR" -type f ! -name "*.*" | while read -r bin; do
        if file "$bin" | grep -q "Mach-O"; then
            codesign --force --sign - "$bin" 2>/dev/null || echo "  Warning: failed to sign $(basename "$bin")"
            echo "  Signed $(basename "$bin")"
        fi
    done

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
