#!/bin/bash
# ==========================================
#  Fix Quarantine - awawapp
# ==========================================
#  This script removes the macOS quarantine
#  attribute that causes the "damaged app"
#  or "unidentified developer" error.
# ==========================================

APP_NAME="awawapp.app"

echo ""
echo "╔══════════════════════════════════════╗"
echo "║     awawapp - Fix Quarantine         ║"
echo "╚══════════════════════════════════════╝"
echo ""

# Check common locations
if [ -d "/Applications/$APP_NAME" ]; then
    APP_PATH="/Applications/$APP_NAME"
    echo "Found app in: /Applications"
elif [ -d "$HOME/Applications/$APP_NAME" ]; then
    APP_PATH="$HOME/Applications/$APP_NAME"
    echo "Found app in: ~/Applications"
elif [ -d "/Volumes/awawapp/$APP_NAME" ]; then
    APP_PATH="/Volumes/awawapp/$APP_NAME"
    echo "Found app in: DMG volume"
else
    echo "❌ Could not find awawapp.app"
    echo ""
    echo "Please copy awawapp.app to /Applications first,"
    echo "then run this script again."
    echo ""
    exit 1
fi

echo ""
echo "Removing quarantine from: $APP_PATH"
echo ""

# Remove quarantine attribute recursively
xattr -cr "$APP_PATH"

if [ $? -eq 0 ]; then
    echo "✅ Success! Quarantine attribute removed."
    echo ""
    echo "You can now open awawapp normally."
else
    echo "❌ Failed to remove quarantine."
    echo ""
    echo "Try running with sudo:"
    echo "  sudo xattr -cr \"$APP_PATH\""
fi

echo ""
