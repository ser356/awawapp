#!/bin/bash
# bundle-mpv.sh — Copy mpv binary for bundling in awawapp
#
# This script copies mpv from Homebrew to the Tauri binaries directory.
#
# Usage:
#   ./scripts/bundle-mpv.sh
#
# Prerequisites:
#   brew install mpv
#
# After running, you should have:
#   - src-tauri/binaries/mpv-{target-triple}
#
# Note: This copies just the mpv binary. The binary will link against
# system-installed dylibs. For full redistribution, users need mpv
# installed via Homebrew, or the app will fall back to detecting
# system-installed mpv.
#
# For a fully standalone bundle, use bundle-mpv-full.sh (requires
# dylibbundler or manual dylib handling).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BINARIES_DIR="$PROJECT_ROOT/src-tauri/binaries"

# Detect architecture
ARCH="$(uname -m)"
if [ "$ARCH" = "arm64" ]; then
    TARGET_TRIPLE="aarch64-apple-darwin"
else
    TARGET_TRIPLE="x86_64-apple-darwin"
fi

echo "=== bundling mpv for awawapp (${TARGET_TRIPLE}) ==="

# Find mpv binary
MPV_BIN="$(which mpv || true)"
if [ -z "$MPV_BIN" ]; then
    echo "❌ mpv not found. Please install it first:"
    echo "   brew install mpv"
    exit 1
fi

echo "✓ Found mpv at: $MPV_BIN"

# Create directories
mkdir -p "$BINARIES_DIR"

# Copy mpv binary with Tauri naming convention
MPV_DEST="$BINARIES_DIR/mpv-${TARGET_TRIPLE}"
echo "→ Copying mpv binary to: $MPV_DEST"
cp "$MPV_BIN" "$MPV_DEST"
chmod +x "$MPV_DEST"

# Ad-hoc code sign
echo "→ Code signing..."
codesign --force --sign - "$MPV_DEST" 2>/dev/null || echo "   ⚠️  Signing failed (may need manual signing)"

# List dependencies (informational)
echo ""
echo "=== mpv dependencies (informational) ==="
otool -L "$MPV_BIN" | grep -v '/System/' | grep -v '/usr/lib/' | head -20 || true

echo ""
echo "=== Bundle complete ==="
echo "Binary: $MPV_DEST"
echo ""
echo "Note: This bundled mpv binary links against Homebrew dylibs."
echo "For the app to work without mpv installed, you would need to:"
echo "  1. Use dylibbundler to copy and relink all dependencies"
echo "  2. Or bundle mpv as a .framework"
echo ""
echo "For development/testing, the app will:"
echo "  1. Try to use the bundled binary first"
echo "  2. Fall back to system-installed mpv in PATH"
echo ""
echo "Next steps:"
echo "  npm run tauri dev"
