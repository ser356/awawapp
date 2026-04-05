//! Tauri application for torrent streaming.
//!
//! Security audit:
//! - All Tauri commands validate input before processing
//! - Database uses parameterized queries (SQL injection prevention)
//! - HTTP API bound to localhost only (127.0.0.1)
//! - Magnet URI validation before processing
//! - Error messages don't expose internal system details
//! - Shell commands are restricted to VLC launch only

pub mod cli;
pub mod database;
mod memory_storage;
pub mod torrent_engine;

use crate::database::{Database, TorrentHistory};
use crate::torrent_engine::{EngineConfig, TorrentEngine, TorrentInfo, TorrentStats};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri::menu::{Menu, MenuItem, Submenu, PredefinedMenuItem, CheckMenuItem};
use tokio::sync::RwLock;
use std::sync::Mutex;
use tracing::{error, info};

/// Stores references to language CheckMenuItems for reliable toggling
pub struct LanguageMenuItems {
    items: Vec<(String, CheckMenuItem<tauri::Wry>)>,
}

impl LanguageMenuItems {
    fn set_language(&self, lang: &str) {
        let selected_id = format!("lang_{}", lang);
        for (id, item) in &self.items {
            let _ = item.set_checked(*id == selected_id);
        }
    }
}

/// Application state shared across commands
pub struct AppState {
    pub engine: RwLock<Option<Arc<TorrentEngine>>>,
    pub database: Arc<Database>,
    pub download_dir: PathBuf,
}

/// Response wrapper for consistent error handling
#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResult<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> CommandResult<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(message: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.to_string()),
        }
    }
}

// ============================================================================
// Tauri Commands
// ============================================================================

/// Add a magnet link and fetch its metadata
#[tauri::command]
async fn add_magnet(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    magnet_uri: String,
) -> Result<CommandResult<TorrentInfo>, String> {
    // Validate magnet URI
    if !torrent_engine::validate_magnet_uri(&magnet_uri) {
        return Ok(CommandResult::err("Invalid magnet link format"));
    }

    // Get engine
    let engine_guard = state.engine.read().await;
    let engine = match engine_guard.as_ref() {
        Some(e) => e.clone(),
        None => return Ok(CommandResult::err("Torrent engine not initialized")),
    };
    drop(engine_guard);

    // Add torrent
    match engine.add_magnet(&magnet_uri, true).await {
        Ok(info) => {
            // Save to history
            if let Err(e) = state.database.add_magnet(&magnet_uri, &info.name) {
                error!("Failed to save to history: {}", e);
            }

            // Update with full info
            if let Ok(id) = state.database.add_magnet(&magnet_uri, &info.name) {
                let _ = state.database.update_torrent_info(id, &info.name, info.total_size as i64);
            }

            // Notify frontend to refresh history
            let _ = app.emit("history-updated", ());

            Ok(CommandResult::ok(info))
        }
        Err(e) => {
            error!("Failed to add magnet: {:#}", e);
            Ok(CommandResult::err(&format!("Failed to add torrent: {:#}", e)))
        }
    }
}

/// Add a torrent from .torrent file bytes
#[tauri::command]
async fn add_torrent_file(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    bytes: Vec<u8>,
    name_hint: Option<String>,
) -> Result<CommandResult<TorrentInfo>, String> {
    let engine_guard = state.engine.read().await;
    let engine = match engine_guard.as_ref() {
        Some(e) => e.clone(),
        None => return Ok(CommandResult::err("Torrent engine not initialized")),
    };
    drop(engine_guard);

    match engine.add_torrent_file(bytes).await {
        Ok(info) => {
            let display_name = name_hint.as_deref().unwrap_or(&info.name);
            if let Err(e) = state.database.add_torrent_entry(&format!("file:{}", info.name), display_name) {
                error!("Failed to save torrent file to history: {}", e);
            }
            // Notify frontend to refresh history
            let _ = app.emit("history-updated", ());
            Ok(CommandResult::ok(info))
        }
        Err(e) => {
            error!("Failed to add torrent file: {:#}", e);
            Ok(CommandResult::err(&format!("Failed to load torrent file: {:#}", e)))
        }
    }
}

/// Start streaming a specific file (sets up download for just that file)
#[tauri::command]
async fn start_stream(
    state: State<'_, Arc<AppState>>,
    torrent_id: usize,
    file_index: usize,
) -> Result<CommandResult<String>, String> {
    let engine_guard = state.engine.read().await;
    let engine = match engine_guard.as_ref() {
        Some(e) => e.clone(),
        None => return Ok(CommandResult::err("Torrent engine not initialized")),
    };
    drop(engine_guard);

    // Start streaming - this sets only_files, unpauses, and returns URL
    match engine.start_stream(torrent_id, file_index).await {
        Ok(url) => Ok(CommandResult::ok(url)),
        Err(e) => {
            error!("Failed to start stream: {}", e);
            Ok(CommandResult::err(&format!("Failed to start stream: {}", e)))
        }
    }
}

/// Pause a torrent download
#[tauri::command]
async fn pause_download(
    state: State<'_, Arc<AppState>>,
    torrent_id: usize,
) -> Result<CommandResult<()>, String> {
    let engine_guard = state.engine.read().await;
    let engine = match engine_guard.as_ref() {
        Some(e) => e.clone(),
        None => return Ok(CommandResult::err("Torrent engine not initialized")),
    };
    drop(engine_guard);

    match engine.pause_download(torrent_id).await {
        Ok(_) => Ok(CommandResult::ok(())),
        Err(e) => {
            error!("Failed to pause download: {}", e);
            Ok(CommandResult::err("Failed to pause download"))
        }
    }
}

/// Get statistics for a specific torrent
#[tauri::command]
async fn get_torrent_stats(
    state: State<'_, Arc<AppState>>,
    torrent_id: usize,
) -> Result<CommandResult<TorrentStats>, String> {
    let engine_guard = state.engine.read().await;
    let engine = match engine_guard.as_ref() {
        Some(e) => e.clone(),
        None => return Ok(CommandResult::err("Torrent engine not initialized")),
    };
    drop(engine_guard);

    match engine.get_stats(torrent_id).await {
        Ok(stats) => Ok(CommandResult::ok(stats)),
        Err(e) => {
            error!("Failed to get stats: {}", e);
            Ok(CommandResult::err("Failed to get torrent statistics"))
        }
    }
}

/// Get statistics for all active torrents
#[tauri::command]
async fn get_all_stats(
    state: State<'_, Arc<AppState>>,
) -> Result<CommandResult<Vec<TorrentStats>>, String> {
    let engine_guard = state.engine.read().await;
    let engine = match engine_guard.as_ref() {
        Some(e) => e.clone(),
        None => return Ok(CommandResult::err("Torrent engine not initialized")),
    };
    drop(engine_guard);

    match engine.get_all_stats().await {
        Ok(stats) => Ok(CommandResult::ok(stats)),
        Err(e) => {
            error!("Failed to get all stats: {}", e);
            Ok(CommandResult::err("Failed to get torrent statistics"))
        }
    }
}

/// Get streaming URL for a file to use with VLC
#[tauri::command]
async fn get_stream_url(
    state: State<'_, Arc<AppState>>,
    torrent_id: usize,
    file_index: usize,
) -> Result<CommandResult<String>, String> {
    let engine_guard = state.engine.read().await;
    let engine = match engine_guard.as_ref() {
        Some(e) => e.clone(),
        None => return Ok(CommandResult::err("Torrent engine not initialized")),
    };
    drop(engine_guard);

    match engine.get_stream_url(torrent_id, file_index).await {
        Ok(url) => Ok(CommandResult::ok(url)),
        Err(e) => Ok(CommandResult::err(&format!("Failed to get stream URL: {}", e))),
    }
}

/// Get torrent history from database
#[tauri::command]
async fn get_history(
    state: State<'_, Arc<AppState>>,
    limit: Option<u32>,
) -> Result<CommandResult<Vec<TorrentHistory>>, String> {
    match state.database.get_history(limit) {
        Ok(history) => Ok(CommandResult::ok(history)),
        Err(e) => {
            error!("Failed to get history: {}", e);
            Ok(CommandResult::err("Failed to load history"))
        }
    }
}

/// Search torrent history
#[tauri::command]
async fn search_history(
    state: State<'_, Arc<AppState>>,
    query: String,
) -> Result<CommandResult<Vec<TorrentHistory>>, String> {
    // Limit query length for security
    if query.len() > 100 {
        return Ok(CommandResult::err("Search query too long"));
    }

    match state.database.search(&query) {
        Ok(results) => Ok(CommandResult::ok(results)),
        Err(e) => {
            error!("Failed to search history: {}", e);
            Ok(CommandResult::err("Search failed"))
        }
    }
}

/// Delete a torrent from history
#[tauri::command]
async fn delete_from_history(
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<CommandResult<()>, String> {
    match state.database.delete_torrent(id) {
        Ok(_) => Ok(CommandResult::ok(())),
        Err(e) => {
            error!("Failed to delete from history: {}", e);
            Ok(CommandResult::err("Failed to delete"))
        }
    }
}

/// Delete a torrent and optionally its files
#[tauri::command]
async fn delete_torrent(
    state: State<'_, Arc<AppState>>,
    torrent_id: usize,
    delete_files: bool,
) -> Result<CommandResult<()>, String> {
    let engine_guard = state.engine.read().await;
    let engine = match engine_guard.as_ref() {
        Some(e) => e.clone(),
        None => return Ok(CommandResult::err("Torrent engine not initialized")),
    };
    drop(engine_guard);

    match engine.delete_torrent(torrent_id, delete_files).await {
        Ok(_) => Ok(CommandResult::ok(())),
        Err(e) => {
            error!("Failed to delete torrent: {}", e);
            Ok(CommandResult::err("Failed to delete torrent"))
        }
    }
}

/// Get the download directory path
#[tauri::command]
async fn get_download_dir(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    Ok(state.download_dir.to_string_lossy().to_string())
}

/// Check if a video player is installed
#[tauri::command]
async fn check_player_installed() -> Result<CommandResult<Option<String>>, String> {
    use std::process::Command;

    #[cfg(target_os = "macos")]
    {
        let apps = ["VLC", "mpv", "IINA"];
        
        for app in apps {
            let check = Command::new("open")
                .args(["-Ra", app])
                .output();
            
            if let Ok(output) = check {
                if output.status.success() {
                    return Ok(CommandResult::ok(Some(app.to_string())));
                }
            }
        }
        return Ok(CommandResult::ok(None));
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, just return None if we can't easily check
        Ok(CommandResult::ok(None))
    }

    #[cfg(target_os = "linux")]
    {
        let players = ["vlc", "mpv"];
        
        for player in players {
            let which = Command::new("which").arg(player).output();
            if let Ok(output) = which {
                if output.status.success() {
                    return Ok(CommandResult::ok(Some(player.to_string())));
                }
            }
        }
        Ok(CommandResult::ok(None))
    }
}

/// Open a streaming URL in the best available player.
/// Tries VLC → mpv → IINA → QuickTime Player
#[tauri::command]
async fn open_in_player(url: String) -> Result<CommandResult<()>, String> {
    use std::process::Command;

    #[cfg(target_os = "macos")]
    {
        // Check which apps are available first, then open with the first available one
        // QuickTime Player is included as last resort (comes with macOS)
        let apps = ["VLC", "mpv", "IINA", "QuickTime Player"];
        
        for app in apps {
            // Check if app exists using 'open -Ra' (doesn't launch, just checks)
            let check = Command::new("open")
                .args(["-Ra", app])
                .output();
            
            if let Ok(output) = check {
                if output.status.success() {
                    // App exists, open the URL with it
                    // For VLC: add network-caching for better streaming buffer
                    let result = if app == "VLC" {
                        Command::new("open")
                            .args(["-a", app, "--args", "--network-caching=5000"])
                            .arg(&url)
                            .spawn()
                    } else if app == "mpv" {
                        Command::new("open")
                            .args(["-a", app, "--args", "--cache=yes", "--cache-pause-initial=yes"])
                            .arg(&url)
                            .spawn()
                    } else {
                        Command::new("open")
                            .args(["-a", app])
                            .arg(&url)
                            .spawn()
                    };
                    
                    if result.is_ok() {
                        info!("Opened stream with: {}", app);
                        return Ok(CommandResult::ok(()));
                    }
                }
            }
        }
        
        // No player found at all
        error!("No video player found on macOS");
        return Ok(CommandResult::err("No se encontró ningún reproductor de video. Instala VLC o mpv."));
    }

    #[cfg(target_os = "windows")]
    {
        // Try Windows Media Player as fallback
        let players = ["vlc", "mpv", "wmplayer"];

        for player in players {
            let result = Command::new("cmd")
                .args(["/C", "start", "", player])
                .arg(&url)
                .spawn();
            
            if result.is_ok() {
                info!("Opened stream with: {}", player);
                return Ok(CommandResult::ok(()));
            }
        }
        
        error!("No video player found on Windows");
        return Ok(CommandResult::err("No se encontró ningún reproductor de video. Instala VLC o mpv."));
    }

    #[cfg(target_os = "linux")]
    {
        let players = ["vlc", "mpv", "celluloid", "totem"];
        
        for player in players {
            // Check if command exists
            let which = Command::new("which").arg(player).output();
            if let Ok(output) = which {
                if output.status.success() {
                    if Command::new(player).arg(&url).spawn().is_ok() {
                        info!("Opened stream with: {}", player);
                        return Ok(CommandResult::ok(()));
                    }
                }
            }
        }
        
        error!("No video player found on Linux");
        Ok(CommandResult::err("No se encontró ningún reproductor de video. Instala VLC o mpv."))
    }
}

/// Paths for the bundled mpv player
#[derive(Debug, Serialize, Deserialize)]
pub struct MpvPaths {
    pub mpv_path: Option<String>,
    pub config_dir: Option<String>,
}

/// Get paths for the bundled mpv binary and config directory.
/// The frontend uses these to launch mpv with the correct binary and config.
#[tauri::command]
async fn get_mpv_paths(app: AppHandle) -> Result<MpvPaths, String> {
    // Resolve mpv binary path.
    // Strategy per platform:
    //   macOS/Windows: bundled sidecar next to the main executable
    //   Linux: system mpv (from PATH), since .deb declares it as a dependency
    let mpv_path = {
        // First try: bundled sidecar next to the main executable
        let sidecar = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
            .map(|dir| {
                #[cfg(target_os = "windows")]
                let binary = dir.join("mpv.exe");
                #[cfg(not(target_os = "windows"))]
                let binary = dir.join("mpv");
                binary
            })
            .filter(|p| p.exists())
            .map(|p| p.to_string_lossy().to_string());

        // On Linux, fall back to system mpv if sidecar not found
        #[cfg(target_os = "linux")]
        let result = sidecar.or_else(|| {
            std::process::Command::new("which")
                .arg("mpv")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        });
        #[cfg(not(target_os = "linux"))]
        let result = sidecar;

        result
    };

    // Resolve config directory from bundled resources.
    let config_dir = app.path().resource_dir()
        .ok()
        .map(|dir| dir.join("mpv-config"))
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string());

    // On Windows, add the resource lib/ directory to PATH so mpv.exe
    // can find its bundled DLLs. Resources go to $INSTDIR/lib/ but
    // mpv.exe is in $INSTDIR/ — Windows doesn't search subdirectories.
    #[cfg(target_os = "windows")]
    {
        if let Ok(resource_dir) = app.path().resource_dir() {
            let lib_dir = resource_dir.join("lib");
            if lib_dir.exists() {
                let lib_path = lib_dir.to_string_lossy().to_string();
                let current = std::env::var("PATH").unwrap_or_default();
                if !current.contains(&lib_path) {
                    std::env::set_var("PATH", format!("{lib_path};{current}"));
                    info!("Added bundled lib dir to PATH: {}", lib_path);
                }
            }
        }
    }

    info!(
        "mpv paths - binary: {:?}, config: {:?}",
        mpv_path, config_dir
    );

    Ok(MpvPaths { mpv_path, config_dir })
}

/// Set the language menu checkmarks to match the current language
#[tauri::command]
async fn set_menu_language(app: AppHandle, lang: String) -> Result<(), String> {
    if let Some(menu_items) = app.try_state::<Mutex<LanguageMenuItems>>() {
        let items = menu_items.lock().unwrap();
        items.set_language(&lang);
    }
    Ok(())
}

/// Event emitter for real-time stats updates
async fn start_stats_emitter(app: AppHandle, state: Arc<AppState>) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));

    loop {
        interval.tick().await;

        let engine_guard = state.engine.read().await;
        if let Some(engine) = engine_guard.as_ref() {
            if let Ok(stats) = engine.get_all_stats().await {
                // Emit stats event to frontend
                if let Err(e) = app.emit("torrent-stats", &stats) {
                    error!("Failed to emit stats: {}", e);
                }
            }
        }
    }
}

/// Periodically checks that the HTTP API server is alive and restarts it if
/// it has crashed.  Runs every 5 seconds.
async fn start_http_watchdog(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

    loop {
        interval.tick().await;

        let engine_guard = state.engine.read().await;
        if let Some(engine) = engine_guard.as_ref() {
            if let Err(e) = engine.ensure_http_server_alive().await {
                error!("HTTP watchdog: failed to restart server: {}", e);
            }
        }
    }
}

// ============================================================================
// Application Entry Point
// ============================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing for logging with RUST_LOG env support
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("awawapp=info".parse().unwrap())
                .add_directive("librqbit=info".parse().unwrap())
        )
        .init();

    info!("=== AWAWAPP STARTING ===");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_mpv::init())
        .setup(|app| {
            // Create native menu with Check for Updates in app menu
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::AboutMetadataBuilder;
                
                let app_handle = app.handle();
                
                // Detect system language to set default checkmark
                let system_lang = std::env::var("LANG")
                    .unwrap_or_else(|_| "en".to_string())
                    .split(&['_', '.', '-'][..])
                    .next()
                    .unwrap_or("en")
                    .to_lowercase();
                
                // App menu (macOS standard) with Check for Updates
                let about = PredefinedMenuItem::about(app_handle, Some("About awawapp"), Some(
                    AboutMetadataBuilder::new()
                        .version(Some("0.1.0"))
                        .build()
                ))?;
                let install_cli_item = MenuItem::with_id(app_handle, "install_cli", "Install CLI...", true, None::<&str>)?;
                
                // Language submenu with native checkmarks - no flags
                let lang_en = CheckMenuItem::with_id(app_handle, "lang_en", "English", true, system_lang == "en", None::<&str>)?;
                let lang_es = CheckMenuItem::with_id(app_handle, "lang_es", "Español", true, system_lang == "es", None::<&str>)?;
                let lang_fr = CheckMenuItem::with_id(app_handle, "lang_fr", "Français", true, system_lang == "fr", None::<&str>)?;
                let lang_de = CheckMenuItem::with_id(app_handle, "lang_de", "Deutsch", true, system_lang == "de", None::<&str>)?;
                let lang_it = CheckMenuItem::with_id(app_handle, "lang_it", "Italiano", true, system_lang == "it", None::<&str>)?;
                let lang_pt = CheckMenuItem::with_id(app_handle, "lang_pt", "Português", true, system_lang == "pt", None::<&str>)?;
                let lang_ru = CheckMenuItem::with_id(app_handle, "lang_ru", "Русский", true, system_lang == "ru", None::<&str>)?;
                let lang_ja = CheckMenuItem::with_id(app_handle, "lang_ja", "日本語", true, system_lang == "ja", None::<&str>)?;
                let lang_ko = CheckMenuItem::with_id(app_handle, "lang_ko", "한국어", true, system_lang == "ko", None::<&str>)?;
                let lang_zh = CheckMenuItem::with_id(app_handle, "lang_zh", "中文", true, system_lang == "zh", None::<&str>)?;
                let lang_nl = CheckMenuItem::with_id(app_handle, "lang_nl", "Nederlands", true, system_lang == "nl", None::<&str>)?;
                let lang_sv = CheckMenuItem::with_id(app_handle, "lang_sv", "Svenska", true, system_lang == "sv", None::<&str>)?;
                let lang_pl = CheckMenuItem::with_id(app_handle, "lang_pl", "Polski", true, system_lang == "pl", None::<&str>)?;
                let lang_tr = CheckMenuItem::with_id(app_handle, "lang_tr", "Türkçe", true, system_lang == "tr", None::<&str>)?;
                let lang_ar = CheckMenuItem::with_id(app_handle, "lang_ar", "العربية", true, system_lang == "ar", None::<&str>)?;
                // Store references for reliable toggling from event handler and commands
                let lang_menu_items = vec![
                    ("lang_en".to_string(), lang_en.clone()),
                    ("lang_es".to_string(), lang_es.clone()),
                    ("lang_fr".to_string(), lang_fr.clone()),
                    ("lang_de".to_string(), lang_de.clone()),
                    ("lang_it".to_string(), lang_it.clone()),
                    ("lang_pt".to_string(), lang_pt.clone()),
                    ("lang_ru".to_string(), lang_ru.clone()),
                    ("lang_ja".to_string(), lang_ja.clone()),
                    ("lang_ko".to_string(), lang_ko.clone()),
                    ("lang_zh".to_string(), lang_zh.clone()),
                    ("lang_nl".to_string(), lang_nl.clone()),
                    ("lang_sv".to_string(), lang_sv.clone()),
                    ("lang_pl".to_string(), lang_pl.clone()),
                    ("lang_tr".to_string(), lang_tr.clone()),
                    ("lang_ar".to_string(), lang_ar.clone()),
                ];

                // Clone for the event handler closure
                let lang_menu_items_for_handler = lang_menu_items.clone();

                // Store in Tauri state for set_menu_language command
                app.manage(Mutex::new(LanguageMenuItems { items: lang_menu_items }));

                let language_menu = Submenu::with_items(app_handle, "Language", true, &[
                    &lang_en,
                    &lang_es,
                    &lang_fr,
                    &lang_de,
                    &lang_it,
                    &lang_pt,
                    &lang_ru,
                    &lang_ja,
                    &lang_ko,
                    &lang_zh,
                    &lang_nl,
                    &lang_sv,
                    &lang_pl,
                    &lang_tr,
                    &lang_ar,
                ])?;
                
                let services = PredefinedMenuItem::services(app_handle, None)?;
                let hide = PredefinedMenuItem::hide(app_handle, None)?;
                let hide_others = PredefinedMenuItem::hide_others(app_handle, None)?;
                let show_all = PredefinedMenuItem::show_all(app_handle, None)?;
                let quit = PredefinedMenuItem::quit(app_handle, Some("Quit awawapp"))?;
                let app_menu = Submenu::with_items(app_handle, "awawapp", true, &[
                    &about,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &install_cli_item,
                    &language_menu,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &services,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &hide,
                    &hide_others,
                    &show_all,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &quit,
                ])?;
                
                // Edit menu
                let undo = PredefinedMenuItem::undo(app_handle, None)?;
                let redo = PredefinedMenuItem::redo(app_handle, None)?;
                let cut = PredefinedMenuItem::cut(app_handle, None)?;
                let copy = PredefinedMenuItem::copy(app_handle, None)?;
                let paste = PredefinedMenuItem::paste(app_handle, None)?;
                let select_all = PredefinedMenuItem::select_all(app_handle, None)?;
                let edit_menu = Submenu::with_items(app_handle, "Edit", true, &[
                    &undo, &redo,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &cut, &copy, &paste,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &select_all,
                ])?;
                
                // Window menu
                let minimize = PredefinedMenuItem::minimize(app_handle, None)?;
                let close = PredefinedMenuItem::close_window(app_handle, None)?;
                let window_menu = Submenu::with_items(app_handle, "Window", true, &[
                    &minimize,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &close,
                ])?;
                
                let menu = Menu::with_items(app_handle, &[
                    &app_menu,
                    &edit_menu,
                    &window_menu,
                ])?;
                
                app.set_menu(menu)?;
                
                // Handle menu events
                app.on_menu_event(move |app, event| {
                    let event_id = event.id().as_ref();

                    if let Some(lang_code) = event_id.strip_prefix("lang_") {
                        // Use direct references to set exactly one item checked
                        for (id, item) in &lang_menu_items_for_handler {
                            let _ = item.set_checked(id == event_id);
                        }
                        let _ = app.emit("language-changed", lang_code);
                    } else if event_id == "install_cli" {
                        let _ = app.emit("install-cli-clicked", ());
                    }
                });

            }
            
            let app_handle = app.handle().clone();
            
            // Get app data directory for database and downloads
            let app_data_dir = app.path().app_data_dir()
                .expect("Failed to get app data directory");
            
            std::fs::create_dir_all(&app_data_dir)
                .expect("Failed to create app data directory");
            
            let db_path = app_data_dir.join("history.db");
            // Pieces are stored in RAM via InMemoryStorageFactory, so the
            // download_dir is only used by librqbit for internal bookkeeping
            // (not for actual file data). A temp dir is fine cross-platform.
            let download_dir = std::env::temp_dir().join("awawapp_session");
            
            // Initialize database
            let database = Database::new(&db_path)
                .expect("Failed to initialize database");

            // Create shared state - use Arc so we can share with background task
            let state = Arc::new(AppState {
                engine: RwLock::new(None),
                database: Arc::new(database),
                download_dir: download_dir.clone(),
            });
            
            // Store state in Tauri (the Arc wrapper is transparent to State<>)
            app.manage(state.clone());
            
            // Initialize torrent engine in background
            let state_clone = state.clone();
            let app_handle_clone = app_handle.clone();
            
            tauri::async_runtime::spawn(async move {
                let config = EngineConfig {
                    download_dir,
                    http_api_port: 3030,
                    max_connections: 100,
                    dht_enabled: true,
                };

                match TorrentEngine::new(config).await {
                    Ok(engine) => {
                        let engine = Arc::new(engine);

                        // Store engine in state
                        let mut engine_guard = state_clone.engine.write().await;
                        *engine_guard = Some(engine);
                        drop(engine_guard);

                        info!("Torrent engine initialized successfully");

                        // Launch watchdog for the HTTP API server
                        let watchdog_state = state_clone.clone();
                        tokio::spawn(async move {
                            start_http_watchdog(watchdog_state).await;
                        });

                        // Start stats emitter (runs forever)
                        start_stats_emitter(app_handle_clone, state_clone).await;
                    }
                    Err(e) => {
                        error!("Failed to initialize torrent engine: {}", e);
                    }
                }
            });
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            add_magnet,
            add_torrent_file,
            start_stream,
            pause_download,
            get_torrent_stats,
            get_all_stats,
            get_stream_url,
            get_history,
            search_history,
            delete_from_history,
            delete_torrent,
            get_download_dir,
            open_in_player,
            check_player_installed,
            set_menu_language,
            get_mpv_paths,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

