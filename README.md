# awawapp

Lightweight torrent streaming app for macOS, Windows, and Linux. Stream torrents directly to VLC without waiting for the full download.

## Features

- **Magnet links & .torrent files** — Paste a magnet link or drag & drop a .torrent file
- **Stream to VLC** — Start watching immediately while the download continues
- **File selection** — Choose which files to stream from multi-file torrents
- **Real-time stats** — Track download progress, speed, and connected peers
- **History** — Keep track of your torrents with searchable history
- **Lightweight** — ~10 MB bundle, ~40 MB RAM usage

## Requirements

- [VLC](https://www.videolan.org/vlc/) installed for video playback

## Installation

### macOS

Download the latest `.dmg` from [Releases](../../releases) and drag to Applications.

### Windows

Download the latest `.msi` or `.exe` installer from [Releases](../../releases).

### Linux

Download the `.AppImage`, `.deb`, or `.rpm` from [Releases](../../releases).

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://rustup.rs/) 1.70+

### Setup

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Tech Stack

- **Frontend:** Vue 3 + TypeScript + PrimeVue
- **Backend:** Rust + Tauri 2
- **Torrent engine:** librqbit
- **Database:** SQLite (rusqlite)

## License

MIT