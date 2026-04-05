#!/bin/bash
# Build the awaw CLI binary for bundling with Tauri
# This script is called before Tauri bundles the app

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
TAURI_DIR="$PROJECT_ROOT/src-tauri"
BINARIES_DIR="$TAURI_DIR/binaries"

# Detect target triple
TARGET_TRIPLE=$(rustc -vV | grep host | cut -d ' ' -f2)

echo "Building awaw CLI for $TARGET_TRIPLE..."

# Ensure binaries directory exists with placeholders
# (Tauri build.rs checks for sidecar files before cargo compiles)
mkdir -p "$BINARIES_DIR"
BINARY_PATH="$BINARIES_DIR/awaw-$TARGET_TRIPLE"
MPV_BINARY_PATH="$BINARIES_DIR/mpv-$TARGET_TRIPLE"

if [ ! -f "$BINARY_PATH" ]; then
    touch "$BINARY_PATH"
fi
if [ ! -f "$MPV_BINARY_PATH" ]; then
    touch "$MPV_BINARY_PATH"
fi

# Build the CLI in release mode
cd "$TAURI_DIR"
cargo build --release --bin awaw

# Copy to binaries directory with target triple suffix
cp "target/release/awaw" "$BINARY_PATH"

echo "CLI binary ready: $BINARY_PATH ($(du -h "$BINARY_PATH" | cut -f1))"
