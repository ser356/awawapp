# Building a macOS torrent streaming app: the definitive framework comparison

**Tauri with librqbit is the strongest choice for this project — and it's already been proven.** The rqbit project (a Rust BitTorrent client) ships an official Tauri desktop app that implements nearly every feature on your list: magnet link input, HTTP streaming with VLC, file selection, and real-time stats. This makes it both the best technical option and a ready-made reference implementation. The broader landscape reveals a clear industry shift away from Electron toward lightweight WebView-based shells backed by Rust, with every major torrent streaming project (rqbit, new Popcorn Time, Stremio) moving in this direction.

---

## The rqbit Tauri app changes the calculus entirely

The single most important finding in this research is that **rqbit already ships a Tauri desktop app** — the exact architecture being evaluated. The desktop app lives in `/desktop/src-tauri/` of the rqbit repo (github.com/ikatson/rqbit, ~1.6k stars), described as "a thin wrapper on top of the Web UI frontend." The librqbit Rust crate is **directly linked** into the Tauri binary — no subprocess, no HTTP bridge, just native Rust function calls via `#[tauri::command]`. The backend exposes a full REST API at `http://localhost:3030` with Range header support for seeking, smart piece prioritization for streaming, and DLNA device advertising. Memory usage sits at "a few tens of megabytes." Pre-built releases exist for macOS and Windows.

This means a developer starting today can fork rqbit's desktop app and customize it, or study its architecture and build something new with the same stack. The Tauri commands in the source show exactly how to wire up torrent creation from magnet links, file selection, streaming endpoints, and progress reporting. **You don't need to figure out the architecture — it's already been built and tested.**

For the Go path, anacrolix/torrent (~5.9k stars) is equally mature but lacks an equivalent desktop GUI reference. Its companion project **confluence** serves as a "torrent client as HTTP service" with streaming endpoints (e.g., `GET /data?magnet=...&path=file.mp4`) and VLC integration documented in its README. Multiple downstream projects (Gopeed, TorrServ, distribyted) validate the library at scale.

---

## Framework-by-framework comparison with real numbers

### Tauri — the lightweight Rust-native winner

Tauri v2.0 shipped stable in October 2024, independently audited, and now sits at **~85k GitHub stars**. On macOS it uses the system WKWebView, producing **3–10 MB bundles** and consuming **~30–40 MB RAM at idle**. Startup is sub-500ms. macOS integration is comprehensive: native menu bar (MenuBuilder API), notifications via plugin, system tray, dock icon progress bars (`set_progress_bar`), automatic dark mode inheritance, code signing, notarization, DMG creation, and universal binary support.

The backend-frontend communication model is clean. Rust functions annotated with `#[tauri::command]` become callable from JavaScript via `invoke()`. A bidirectional event system handles real-time updates (perfect for progress bars and peer counts). Thread-safe state management uses `tauri::State<T>` with Mutex/RwLock. The frontend can be React, Svelte, Vue, or vanilla JS with Vite hot-reload. Initial Rust compile takes ~80 seconds, but incremental builds are fast.

For the four required features, Tauri delivers cleanly: magnet paste triggers a Tauri command that calls librqbit directly; file selection uses the torrent metadata API to populate a web-based list; real-time stats flow through the event system at configurable intervals; persistent history uses `tauri-plugin-store` (key-value) or `tauri-plugin-sql` (SQLite). All of this is demonstrated in the rqbit desktop app.

### Wails — the best Go-native alternative

Wails v2 is stable at ~31.7k stars, also using WKWebView on macOS. Bundles land at **15–30 MB** (larger due to Go binary overhead) with **~30–60 MB RAM** at idle. The key advantage: **anacrolix/torrent integrates as a direct Go import** — `go get github.com/anacrolix/torrent` and call it natively, sharing types and using Go channels for concurrency. Go methods automatically generate JavaScript bindings with TypeScript definitions. A unified event system handles bidirectional communication.

macOS features in v2 include native menus, dialogs, dark/light mode, translucency, notifications, DMG creation, and code signing. **Wails v3 remains in alpha** as of April 2026 with no release date, adding multi-window support and improved system tray handling. The notable gap: **no built-in auto-updater** (community solutions exist but lag behind Tauri's mature plugin).

For Go developers, Wails is the most productive path. The learning curve is lower than Rust/Tauri, and the direct anacrolix/torrent integration avoids all FFI complexity. However, no existing Wails torrent app was found as a reference — you'd build from scratch.

### Electron — proven but heavy

Electron remains the 800-pound gorilla at **~115k stars**, powering VS Code, Slack, Discord, and Figma. But the numbers tell the story: **80–250 MB bundles**, **150–300 MB RAM at idle**, and **1–2 second startup times**. One benchmark measured 409 MB with 6 windows open. For a tool described as needing to be "lightweight," this is a hard sell.

WebTorrent Desktop proves Electron can build torrent streaming apps — it supports in-app playback, VLC launch, AirPlay, and Chromecast. But it's in maintenance mode with slowed development. A Go/Rust backend would require a subprocess or native module bridge, adding architectural complexity that Tauri and Wails avoid entirely.

### Swift + SwiftUI — best native feel, more plumbing required

If macOS is the only target and native feel is paramount, **SwiftUI + Rust via Mozilla UniFFI** is the premier choice. SwiftUI provides native progress bars (`ProgressView`), list views, `@AppStorage` persistence, automatic dark mode, `MenuBarExtra` for menu bar apps (macOS 13+), and dock integration. Bundle size is **10–25 MB total** (SwiftUI + Rust static library). Memory sits at **~30–80 MB** for a modest app.

UniFFI is production-quality, used by Mozilla Firefox and Radix. It generates Swift bindings automatically from Rust proc macros, with clean type mappings: `Result<T, E>` becomes Swift `throws`, `Option<T>` becomes `T?`, enums map to enums. **Rust FFI overhead is ~1–2 nanoseconds per call** — essentially zero for I/O-bound torrent operations. The Ghostty terminal (by Mitchell Hashimoto) validates this architecture pattern at scale: 94% Zig core + 4% Swift UI with a C ABI bridge.

For Go backends, the story is messier. Go's CGO adds **~40ns overhead per call** with goroutine scheduling complexity. The recommended Go approach is an **embedded HTTP server** with WebSocket for streaming updates — simpler than CGO FFI and providing natural streaming support. The Go binary bundles inside the .app and launches as a subprocess.

| Approach | Bundle | RAM (idle) | Backend integration | macOS native feel |
|----------|--------|------------|--------------------|--------------------|
| **Tauri + Rust** | 3–10 MB | 30–40 MB | Direct (same language) | Excellent (WebView) |
| **Wails + Go** | 15–30 MB | 30–60 MB | Direct (same language) | Good–Excellent |
| **SwiftUI + Rust (UniFFI)** | 10–25 MB | 30–80 MB | FFI (~1ns overhead) | Perfect (native) |
| **SwiftUI + Go (HTTP)** | 15–30 MB | 40–80 MB | HTTP/WebSocket IPC | Perfect (native) |
| **Electron** | 80–250 MB | 150–300 MB | Subprocess or native module | Good (but heavy) |

---

## Native Go and Rust GUI frameworks aren't ready yet

A thorough evaluation of native GUI frameworks reveals a sobering reality: **none deliver a macOS-native experience**, and most are pre-1.0. The 2025 boringcactus survey of 43 Rust GUI libraries found that 94.4% aren't production-ready.

**Fyne** (Go, ~28k stars, v2.7 stable) is the most mature option — production-ready with all required widgets (progress bars, lists, text input, built-in persistent storage). Tailscale uses it. Direct anacrolix/torrent integration is trivial since both are Go. However, Fyne renders its own **Material Design widgets**, not native macOS controls. It looks consistent across platforms but never truly macOS-native.

**Iced** (Rust, ~29.5k stars, v0.14) is architecturally excellent with an Elm Architecture that scales well for stateful apps. System76's COSMIC desktop validates it at scale. Version 0.14 added reactive rendering with 60–80% CPU savings. But it's pre-1.0 with a history of breaking API changes, lacks native macOS menu integration, and requires significant Rust expertise.

**Slint** (Rust, ~18k stars) is the only Rust GUI framework at 1.x with a stable API, but it was designed embedded-first. Its team explicitly states they're "working to make Slint production-ready for desktop." **egui** (Rust, ~24k stars) excels for developer tools but uses immediate-mode rendering that wastes CPU for mostly-idle apps. **Gio** (Go) is explicitly experimental and not recommended for production.

**Bubble Tea** (Go TUI, ~38k stars) deserves mention as a dark horse for a developer-facing prototype — a TUI torrent streaming tool would be functional, tiny, and fast to build, though inappropriate for general users.

The bottom line: if you want a native GUI without WebView, use SwiftUI. If you want a cross-platform native GUI, Fyne is the only production-ready option but sacrifices macOS-native aesthetics.

---

## HTTP streaming to VLC is a solved problem

Every successful torrent streaming project converges on the same pattern: **a local HTTP server with Range header support**. The architecture is straightforward:

The torrent engine runs an HTTP server bound to `127.0.0.1` (localhost only, for security). A streaming endpoint serves file data, blocking the response until required pieces are available from the swarm. Pieces near the current read position receive elevated priority. When the user seeks, the torrent engine reprioritizes from the new position. VLC connects via a simple URL: `vlc "http://localhost:3030/stream/path/to/file.mp4"`.

Both candidate libraries implement this natively. **rqbit** serves its streaming API at `http://127.0.0.1:3030/torrents/{id}/stream` with full Range support and smart blocking. **anacrolix/torrent** exposes a `Reader` implementing `io.ReadSeeker` that requests only the data needed to satisfy reads; its companion tool confluence wraps this as `GET /data?magnet=...&path=file.mp4`.

For launching VLC on macOS, the standard approach is: `/Applications/VLC.app/Contents/MacOS/VLC "http://localhost:PORT/stream/path"`. Tauri's shell plugin or Go's `exec.Command` handle this trivially. Consider checking whether VLC is installed and falling back to `open -a VLC` for robustness.

For port selection, use a configurable default (3030 or 8080) with fallback to a random available port. Store the chosen port so the UI and VLC both know where to connect. For persistent magnet link history, **SQLite** is the recommended approach — it handles concurrent access safely (important when both the HTTP server and UI read/write), supports efficient querying and sorting, and provides crash recovery via WAL mode. Both `rusqlite` (Rust) and `crawshaw/sqlite` (Go) are mature bindings.

---

## Lessons from existing projects point to Tauri + Rust

The industry trajectory is unmistakable. **Popcorn Time's latest rebuild migrated from Electron to Tauri with a Rust + React/TypeScript stack.** Stremio's next-generation shell (`stremio-shell-ng`) is being rewritten in Rust with WebView2. WebTorrent Desktop (Electron) is in maintenance mode. rqbit launched natively on Tauri. Every active project is converging on the same architecture: **Rust core logic + web UI in a lightweight native shell**.

The common patterns across all successful projects are worth noting:

- **HTTP API as the universal glue** — both the embedded UI and external players (VLC) consume the same HTTP endpoints
- **Web UI as desktop UI** — wrapping a React/Svelte frontend in a Tauri/WebView2 shell is the dominant pattern
- **Smart piece prioritization** — streaming requires pieces to be downloaded in read-order, not rarest-first
- **Thin shell, thick backend** — rqbit's Tauri app is explicitly "a thin wrapper" over the web UI; all intelligence lives in the Rust library

## Conclusion

**For this specific project, Tauri + librqbit is not just the best option — it's the obvious one.** The rqbit desktop app is an open-source, working implementation of nearly every required feature. Fork it, extend it, or use it as an architectural blueprint. The bundle will be under 10 MB, memory under 40 MB, and the entire torrent engine links directly as a Rust crate with zero bridging overhead.

If Go is strongly preferred, **Wails + anacrolix/torrent** is the clear second choice — direct library integration, stable framework, lightweight WebView shell, just without the benefit of an existing reference app. The main trade-off is a larger bundle (~15–30 MB) and the absence of a built-in auto-updater.

**SwiftUI + Rust (UniFFI)** makes sense only if macOS-exclusive native aesthetics are a hard requirement and the developer is comfortable maintaining both Swift and Rust codebases. The architectural overhead of FFI bridging is real but manageable.

Skip Electron (too heavy for the stated requirements), skip native Go/Rust GUI frameworks (none deliver macOS-native feel), and skip building the streaming infrastructure from scratch (both rqbit and anacrolix/torrent have solved HTTP streaming with Range support and piece prioritization). The hard problems are already solved — the remaining work is UI polish and feature customization.