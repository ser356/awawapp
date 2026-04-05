#!/bin/bash
# bundle-mpv.sh — Bundle mpv + dylibs + uosc for standalone awawapp
#
# This script prepares the mpv sidecar binary, its dynamic libraries,
# and uosc scripts so the final .app is fully self-contained.
#
# Usage:
#   ./scripts/bundle-mpv.sh
#
# What it does:
#   1. Copies mpv binary from system PATH to src-tauri/binaries/
#   2. Bundles all non-system dylibs into src-tauri/lib/ and rewrites paths
#   3. Downloads uosc (mpv modern UI) to src-tauri/mpv-config/scripts/uosc/
#   4. Ad-hoc code signs everything for macOS
#
# Prerequisites (build machine only):
#   brew install mpv
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

# Detect architecture
ARCH="$(uname -m)"
if [ "$ARCH" = "arm64" ]; then
    TARGET_TRIPLE="aarch64-apple-darwin"
else
    TARGET_TRIPLE="x86_64-apple-darwin"
fi

echo "=== Bundle mpv + dylibs + uosc for awawapp (${TARGET_TRIPLE}) ==="
echo ""

# ── Step 1: Bundle mpv binary ───────────────────────────────────────────────

echo "── Step 1: mpv binary ──"

MPV_BIN="$(which mpv || true)"
if [ -z "$MPV_BIN" ]; then
    echo "ERROR: mpv not found in PATH."
    echo "  Install it on the build machine: brew install mpv"
    echo "  (End users will NOT need this — the binary gets bundled.)"
    exit 1
fi

echo "  Found mpv at: $MPV_BIN"

mkdir -p "$BINARIES_DIR"

MPV_DEST="$BINARIES_DIR/mpv-${TARGET_TRIPLE}"
echo "  Copying to: $MPV_DEST"
# Remove old binary — chmod first in case it was code-signed/locked
if [ -f "$MPV_DEST" ]; then
    chmod u+w "$MPV_DEST" 2>/dev/null || true
    rm -f "$MPV_DEST"
fi
cp "$MPV_BIN" "$MPV_DEST"
chmod +x "$MPV_DEST"

echo "  Done."
echo ""

# ── Step 2: Bundle dylibs ──────────────────────────────────────────────────
#
# In the final .app, the layout is:
#   Contents/MacOS/mpv          (sidecar binary)
#   Contents/Resources/lib/     (bundled dylibs)
#
# So dylib paths are rewritten to: @executable_path/../Resources/lib/<name>
#

echo "── Step 2: Bundle dylibs ──"

# Keep libmpv-wrapper.dylib if it exists
WRAPPER_BACKUP=""
if [ -f "$LIB_DIR/libmpv-wrapper.dylib" ]; then
    WRAPPER_BACKUP="$(mktemp)"
    cp "$LIB_DIR/libmpv-wrapper.dylib" "$WRAPPER_BACKUP"
fi

# Clean old bundled dylibs (except wrapper)
rm -rf "$LIB_DIR"
mkdir -p "$LIB_DIR"

# Restore wrapper if it existed
if [ -n "$WRAPPER_BACKUP" ] && [ -f "$WRAPPER_BACKUP" ]; then
    cp "$WRAPPER_BACKUP" "$LIB_DIR/libmpv-wrapper.dylib"
    rm -f "$WRAPPER_BACKUP"
fi

RPATH_PREFIX="@executable_path/../Resources/lib"

# Collect all non-system dylibs recursively
# System dylibs are in /System/ and /usr/lib/ — everything else needs bundling
collect_dylibs() {
    local binary="$1"
    otool -L "$binary" 2>/dev/null | tail -n +2 | awk '{print $1}' | while read -r dylib; do
        # Skip system libraries
        case "$dylib" in
            /System/*|/usr/lib/*|@executable_path/*|@loader_path/*|@rpath/*)
                continue
                ;;
        esac
        echo "$dylib"
    done
}

# Track which dylibs we've already processed to avoid infinite loops
declare -A PROCESSED_DYLIBS

bundle_dylibs_recursive() {
    local binary="$1"
    local binary_name
    binary_name="$(basename "$binary")"

    local dylibs
    dylibs="$(collect_dylibs "$binary")"

    if [ -z "$dylibs" ]; then
        return
    fi

    echo "$dylibs" | while read -r dylib_path; do
        local dylib_name
        dylib_name="$(basename "$dylib_path")"

        # Skip if already processed
        if [ -f "$LIB_DIR/$dylib_name" ]; then
            # Still need to rewrite the reference in the current binary
            install_name_tool -change "$dylib_path" "$RPATH_PREFIX/$dylib_name" "$binary" 2>/dev/null || true
            continue
        fi

        # Resolve the actual dylib path (follow symlinks)
        local real_path="$dylib_path"
        if [ -L "$dylib_path" ]; then
            real_path="$(readlink -f "$dylib_path")"
        fi

        if [ ! -f "$real_path" ]; then
            echo "    WARNING: dylib not found: $dylib_path"
            continue
        fi

        echo "    Bundling: $dylib_name"

        # Copy dylib
        cp "$real_path" "$LIB_DIR/$dylib_name"
        chmod u+w "$LIB_DIR/$dylib_name"

        # Update the dylib's own install name
        install_name_tool -id "$RPATH_PREFIX/$dylib_name" "$LIB_DIR/$dylib_name" 2>/dev/null || true

        # Rewrite the reference in the parent binary
        install_name_tool -change "$dylib_path" "$RPATH_PREFIX/$dylib_name" "$binary" 2>/dev/null || true

        # Recurse into the dylib's own dependencies
        bundle_dylibs_recursive "$LIB_DIR/$dylib_name"
    done
}

echo "  Scanning mpv dependencies..."
bundle_dylibs_recursive "$MPV_DEST"

# Also handle libmpv-wrapper.dylib if present
if [ -f "$LIB_DIR/libmpv-wrapper.dylib" ]; then
    chmod u+w "$LIB_DIR/libmpv-wrapper.dylib"
    echo "  Scanning libmpv-wrapper.dylib dependencies..."
    bundle_dylibs_recursive "$LIB_DIR/libmpv-wrapper.dylib"
fi

DYLIB_COUNT=$(find "$LIB_DIR" -name "*.dylib" | wc -l | tr -d ' ')
echo "  Bundled $DYLIB_COUNT dylibs to: $LIB_DIR"
echo ""

# ── Step 3: Code sign everything ───────────────────────────────────────────

echo "── Step 3: Code signing ──"

# Sign all bundled dylibs first
find "$LIB_DIR" -name "*.dylib" | while read -r lib; do
    codesign --force --sign - "$lib" 2>/dev/null || echo "  Warning: failed to sign $(basename "$lib")"
done
echo "  Signed dylibs."

# Sign mpv binary last
codesign --force --sign - "$MPV_DEST" 2>/dev/null || echo "  Warning: failed to sign mpv"
echo "  Signed mpv."

echo "  Done."
echo ""

# ── Step 4: Bundle uosc scripts ─────────────────────────────────────────────

echo "── Step 4: uosc v${UOSC_VERSION} ──"

UOSC_DIR="$MPV_CONFIG_DIR/scripts/uosc"
UOSC_ZIP_URL="https://github.com/tomasklaen/uosc/releases/download/${UOSC_VERSION}/uosc.zip"
TMP_DIR="$(mktemp -d)"

# Clean previous uosc if exists
if [ -d "$UOSC_DIR" ]; then
    echo "  Removing old uosc..."
    rm -rf "$UOSC_DIR"
fi

echo "  Downloading uosc v${UOSC_VERSION}..."
if curl -fsSL "$UOSC_ZIP_URL" -o "$TMP_DIR/uosc.zip"; then
    echo "  Extracting..."
    unzip -q "$TMP_DIR/uosc.zip" -d "$TMP_DIR/uosc-extracted"

    # uosc zip contains scripts/uosc/ directory structure
    if [ -d "$TMP_DIR/uosc-extracted/scripts/uosc" ]; then
        mkdir -p "$MPV_CONFIG_DIR/scripts"
        cp -R "$TMP_DIR/uosc-extracted/scripts/uosc" "$UOSC_DIR"
        echo "  Installed uosc scripts to: $UOSC_DIR"
    elif [ -d "$TMP_DIR/uosc-extracted/uosc" ]; then
        # Some releases have flat structure
        mkdir -p "$MPV_CONFIG_DIR/scripts"
        cp -R "$TMP_DIR/uosc-extracted/uosc" "$UOSC_DIR"
        echo "  Installed uosc scripts to: $UOSC_DIR"
    else
        echo "  Warning: unexpected uosc zip structure. Contents:"
        ls -la "$TMP_DIR/uosc-extracted/"
        # Try to find and copy whatever lua files exist
        mkdir -p "$UOSC_DIR"
        find "$TMP_DIR/uosc-extracted" -name "*.lua" -exec cp {} "$UOSC_DIR/" \;
        echo "  Copied lua files to: $UOSC_DIR"
    fi

    # If the zip also contains script-opts, don't overwrite ours (we have custom theme)
    echo "  Keeping custom awawapp uosc.conf (not overwriting with default)"
else
    echo "  Warning: Failed to download uosc. Trying local fallback..."
    # Check if uosc is installed locally on the build machine
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

# Cleanup
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

echo "  Bundled dylibs:"
if [ -d "$LIB_DIR" ]; then
    LIB_COUNT=$(find "$LIB_DIR" -name "*.dylib" | wc -l | tr -d ' ')
    LIB_SIZE=$(du -sh "$LIB_DIR" | cut -f1)
    echo "    OK: $LIB_COUNT dylibs ($LIB_SIZE total)"
else
    echo "    MISSING: $LIB_DIR"
fi

echo "  Remaining non-system deps in mpv binary:"
REMAINING=$(otool -L "$MPV_DEST" 2>/dev/null | tail -n +2 | awk '{print $1}' | grep -v '/System/' | grep -v '/usr/lib/' | grep -v '@executable_path' || true)
if [ -z "$REMAINING" ]; then
    echo "    OK: All deps are system or @executable_path (fully portable)"
else
    echo "    WARNING: These deps are NOT bundled yet:"
    echo "$REMAINING" | sed 's/^/      /'
fi

echo "  uosc scripts:"
if [ -d "$UOSC_DIR" ]; then
    UOSC_FILES=$(find "$UOSC_DIR" -name "*.lua" | wc -l | tr -d ' ')
    echo "    OK: $UOSC_DIR ($UOSC_FILES lua files)"
else
    echo "    MISSING: $UOSC_DIR"
fi

echo "  mpv config:"
if [ -f "$MPV_CONFIG_DIR/mpv.conf" ]; then
    echo "    OK: $MPV_CONFIG_DIR/mpv.conf"
else
    echo "    MISSING: $MPV_CONFIG_DIR/mpv.conf"
fi

echo "  uosc config:"
if [ -f "$MPV_CONFIG_DIR/script-opts/uosc.conf" ]; then
    echo "    OK: $MPV_CONFIG_DIR/script-opts/uosc.conf"
else
    echo "    MISSING: $MPV_CONFIG_DIR/script-opts/uosc.conf"
fi

echo ""
echo "=== Bundle complete ==="
echo ""
echo "Next: npm run tauri:build"
