#!/bin/bash
# bundle-mpv.sh — Bundle mpv + uosc for standalone awawapp
#
# This script prepares the mpv sidecar binary and uosc scripts
# so the final .app is fully self-contained (no brew install needed).
#
# Usage:
#   ./scripts/bundle-mpv.sh
#
# What it does:
#   1. Copies mpv binary from system PATH to src-tauri/binaries/
#   2. Downloads uosc (mpv modern UI) to src-tauri/mpv-config/scripts/uosc/
#   3. Ad-hoc code signs the binary for macOS
#
# Prerequisites (build machine only):
#   brew install mpv
#
# After running, the user does NOT need mpv installed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BINARIES_DIR="$PROJECT_ROOT/src-tauri/binaries"
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

echo "=== Bundle mpv + uosc for awawapp (${TARGET_TRIPLE}) ==="
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
rm -f "$MPV_DEST"
cp "$MPV_BIN" "$MPV_DEST"
chmod +x "$MPV_DEST"

# Ad-hoc code sign (required for macOS arm64)
echo "  Code signing..."
codesign --force --sign - "$MPV_DEST" 2>/dev/null || echo "  Warning: signing failed (may need manual signing)"

echo "  Done."
echo ""

# ── Step 2: Bundle uosc scripts ─────────────────────────────────────────────

echo "── Step 2: uosc v${UOSC_VERSION} ──"

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

# ── Step 3: Verify bundle ───────────────────────────────────────────────────

echo "── Verification ──"

echo "  mpv binary:"
if [ -f "$MPV_DEST" ]; then
    echo "    OK: $MPV_DEST ($(du -h "$MPV_DEST" | cut -f1))"
else
    echo "    MISSING: $MPV_DEST"
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
echo "mpv dependencies (for reference):"
otool -L "$MPV_DEST" 2>/dev/null | grep -v '/System/' | grep -v '/usr/lib/' | head -10 || true
echo ""
echo "Note: The bundled mpv links against Homebrew dylibs."
echo "For fully portable builds, use dylibbundler to relink dependencies."
echo ""
echo "Next: npm run tauri:build"
