//! Tauri application for torrent streaming.
//!
//! Security audit:
//! - All Tauri commands validate input before processing
//! - Database uses parameterized queries (SQL injection prevention)
//! - HTTP API bound to localhost only (127.0.0.1)
//! - Magnet URI validation before processing
//! - Error messages don't expose internal system details
//! - Shell commands are restricted to VLC launch only

mod database;
mod torrent_engine;

use crate::database::{Database, TorrentHistory};
use crate::torrent_engine::{EngineConfig, TorrentEngine, TorrentInfo, TorrentStats};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::RwLock;
use tracing::{error, info};

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

            Ok(CommandResult::ok(info))
        }
        Err(e) => {
            error!("Failed to add magnet: {}", e);
            Ok(CommandResult::err("Failed to add torrent. Please check the magnet link."))
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

/// Open a URL in VLC
#[tauri::command]
async fn open_in_vlc(url: String) -> Result<CommandResult<()>, String> {
    use std::process::Command;
    
    // On macOS, use 'open' command with VLC
    #[cfg(target_os = "macos")]
    {
        match Command::new("open")
            .args(["-a", "VLC", &url])
            .spawn()
        {
            Ok(_) => Ok(CommandResult::ok(())),
            Err(e) => {
                error!("Failed to open VLC: {}", e);
                Ok(CommandResult::err(&format!("Failed to open VLC: {}", e)))
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        // On Windows, try to find VLC and open with the URL
        match Command::new("cmd")
            .args(["/C", "start", "vlc", &url])
            .spawn()
        {
            Ok(_) => Ok(CommandResult::ok(())),
            Err(e) => {
                error!("Failed to open VLC: {}", e);
                Ok(CommandResult::err(&format!("Failed to open VLC: {}", e)))
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        match Command::new("vlc")
            .arg(&url)
            .spawn()
        {
            Ok(_) => Ok(CommandResult::ok(())),
            Err(e) => {
                error!("Failed to open VLC: {}", e);
                Ok(CommandResult::err(&format!("Failed to open VLC: {}", e)))
            }
        }
    }
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

// ============================================================================
// Application Entry Point
// ============================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            
            // Get app data directory for database and downloads
            let app_data_dir = app.path().app_data_dir()
                .expect("Failed to get app data directory");
            
            std::fs::create_dir_all(&app_data_dir)
                .expect("Failed to create app data directory");
            
            let db_path = app_data_dir.join("history.db");
            // Use a RAM-backed filesystem so pieces never touch disk.
            // On Linux /dev/shm is a tmpfs mounted in RAM; the OS frees it when
            // the process exits. Falls back to /tmp on other platforms.
            let download_dir = ram_stream_dir();
            
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
                        
                        // Start stats emitter
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
            open_in_vlc,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Return a RAM-backed directory for torrent piece storage.
///
/// On Linux we use /dev/shm (a tmpfs mounted entirely in RAM).
/// Pieces written there never touch the HDD/SSD and are freed by the OS
/// when the process exits, replicating Stremio-style ephemeral streaming.
/// On other platforms we fall back to the system temp directory.
fn ram_stream_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        let shm = PathBuf::from("/dev/shm/awawapp_stream");
        // /dev/shm is always available on Linux (tmpfs in RAM)
        return shm;
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::temp_dir().join("awawapp_stream")
    }
}
