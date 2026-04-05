# awawapp

Lightweight torrent streaming app for macOS. Stream torrents with embedded video playback (like Stremio).

## Features

- **Magnet links & .torrent files** — Paste a magnet link or drag & drop a .torrent file
- **Embedded libmpv playback** — Video plays inside the app window with full codec support (MKV, HEVC, AC3, DTS, ASS subs)
- **File selection** — Choose which files to stream from multi-file torrents
- **Real-time stats** — Track download progress, speed, and connected peers
- **History** — Keep track of your torrents with searchable history
- **Lightweight** — Small bundle, low RAM usage

## Requirements

- **macOS** — Apple Silicon (M1/M2/M3) or Intel

The app bundles libmpv and all required libraries — no extra dependencies needed.

## Installation

### macOS

Download the latest `.dmg` from [Releases](../../releases) and drag to Applications.

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://rustup.rs/) 1.70+
- [mpv](https://mpv.io/) (`brew install mpv`)

### Setup

```bash
# Install dependencies
npm install

# Setup libmpv-wrapper (automatically downloads the wrapper library)
npx tauri-plugin-libmpv-api setup-lib

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Tech Stack

- **Frontend:** Vue 3 + TypeScript + PrimeVue
- **Backend:** Rust + Tauri 2
- **Video:** tauri-plugin-libmpv (embedded libmpv, like Stremio)
- **Torrent engine:** librqbit (in-memory streaming)
- **Database:** SQLite (rusqlite)

## License

MIT