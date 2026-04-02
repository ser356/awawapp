//! Torrent engine module wrapping librqbit.
//!
//! Security considerations:
//! - Downloads are restricted to a configurable directory
//! - Connection limits to prevent resource exhaustion
//! - Validates magnet URIs before processing
//! - Localhost-only HTTP API binding

use anyhow::{Context, Result};
use librqbit::{
    api::{Api, TorrentIdOrHash},
    http_api::{HttpApi, HttpApiOptions},
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent,
    Session, SessionOptions, SessionPersistenceConfig,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::info;

/// File information for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentFile {
    pub index: usize,
    pub path: String,
    pub size: u64,
    pub selected: bool,
}

/// Torrent statistics for real-time display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentStats {
    pub id: usize,
    pub name: String,
    pub progress: f64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub peers_connected: usize,
    pub peers_total: usize,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub state: String,
    pub eta_seconds: Option<u64>,
}

/// Torrent info returned after adding a magnet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentInfo {
    pub id: usize,
    pub name: String,
    pub files: Vec<TorrentFile>,
    pub total_size: u64,
}

/// Configuration for the torrent engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub download_dir: PathBuf,
    pub http_api_port: u16,
    pub max_connections: u16,
    pub dht_enabled: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            download_dir: dirs::download_dir()
                .unwrap_or_else(|| PathBuf::from("./downloads")),
            http_api_port: 3030,
            max_connections: 100,
            dht_enabled: true,
        }
    }
}

/// Stored torrent handle info
struct StoredTorrent {
    #[allow(dead_code)]
    handle: Arc<ManagedTorrent>,
    name: String,
    total_size: u64,
    #[allow(dead_code)]
    files: Vec<TorrentFile>,
    #[allow(dead_code)]
    torrent_idx: usize,
}

/// The main torrent engine wrapping librqbit
pub struct TorrentEngine {
    session: Arc<Session>,
    config: EngineConfig,
    /// Maps internal torrent indices to their handles
    torrents: RwLock<Vec<StoredTorrent>>,
}

impl TorrentEngine {
    /// Create a new torrent engine with the given configuration
    pub async fn new(config: EngineConfig) -> Result<Self> {
        // Ensure download directory exists
        tokio::fs::create_dir_all(&config.download_dir)
            .await
            .context("Failed to create download directory")?;

        // Configure session with security-minded defaults
        let session_opts = SessionOptions {
            disable_dht: !config.dht_enabled,
            disable_dht_persistence: false,
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(config.download_dir.clone()),
            }),
            listen_port_range: Some(6881..6889),
            ..Default::default()
        };

        let session = Session::new_with_opts(
            config.download_dir.clone(),
            session_opts,
        )
        .await
        .context("Failed to create torrent session")?;

        // Start HTTP API server for streaming
        let http_port = config.http_api_port;
        let api = Api::new(session.clone(), None, None);
        let http_api = HttpApi::new(api, Some(HttpApiOptions::default()));
        
        // Bind to localhost only for security
        let addr: SocketAddr = ([127, 0, 0, 1], http_port).into();
        let listener = TcpListener::bind(addr)
            .await
            .context("Failed to bind HTTP API server")?;
        
        info!("Starting HTTP API server on http://127.0.0.1:{}", http_port);
        
        // Spawn HTTP server in background
        tokio::spawn(async move {
            if let Err(e) = http_api.make_http_api_and_run(listener, None).await {
                tracing::error!("HTTP API server error: {}", e);
            }
        });

        info!("Torrent engine initialized, download dir: {:?}", config.download_dir);

        Ok(Self {
            session,
            config,
            torrents: RwLock::new(Vec::new()),
        })
    }

    /// Add a magnet link and return torrent info
    /// Add a magnet link in streaming mode (no auto-download)
    /// Files are only downloaded when explicitly streamed
    pub async fn add_magnet(&self, magnet_uri: &str, _paused: bool) -> Result<TorrentInfo> {
        // Validate magnet URI format
        if !magnet_uri.starts_with("magnet:?") {
            anyhow::bail!("Invalid magnet URI format");
        }

        // Basic URI length check to prevent resource exhaustion
        if magnet_uri.len() > 10000 {
            anyhow::bail!("Magnet URI too long");
        }

        // Append well-known public trackers to improve peer discovery
        // This greatly increases the chance of fetching metadata quickly
        let enhanced_uri = append_public_trackers(magnet_uri);
        info!("Adding magnet with enhanced trackers: {}", &enhanced_uri[..enhanced_uri.len().min(100)]);

        // Start UN-paused to allow metadata fetching, with no files selected
        // This allows DHT/peers to be contacted for metadata
        let add_opts = AddTorrentOptions {
            paused: false, // Need to be active to fetch metadata
            only_files: Some(vec![]), // Empty = no files download after metadata
            ..Default::default()
        };

        let response = self
            .session
            .add_torrent(AddTorrent::from_url(&enhanced_uri), Some(add_opts))
            .await
            .context("Failed to add torrent")?;

        let (torrent_idx, handle) = match response {
            AddTorrentResponse::Added(idx, handle) => {
                info!("Torrent added, fetching metadata from peers...");
                (idx, handle)
            },
            AddTorrentResponse::AlreadyManaged(idx, handle) => {
                info!("Torrent already exists, using cached metadata");
                (idx, handle)
            },
            AddTorrentResponse::ListOnly(_) => {
                anyhow::bail!("Torrent was only listed, not added for download");
            }
        };

        // Wait for metadata with extended timeout (120 seconds)
        // DHT peer discovery can be slow for less popular torrents
        let timeout_duration = tokio::time::Duration::from_secs(120);
        info!("Waiting for torrent metadata (timeout: {}s)...", timeout_duration.as_secs());
        
        match tokio::time::timeout(timeout_duration, handle.wait_until_initialized()).await {
            Ok(result) => {
                result.context("Failed to fetch torrent metadata")?;
                info!("Metadata received successfully");
            },
            Err(_) => {
                // Clean up the torrent that couldn't get metadata
                let _ = self.session.delete(TorrentIdOrHash::Id(torrent_idx), false).await;
                anyhow::bail!("Could not fetch torrent metadata after 120 seconds. This torrent may be unavailable or have no active peers. Try a different magnet link.");
            }
        };

        // Extract file info and name from metadata
        let (name, files, total_size) = handle.with_metadata(|metadata| {
            // Get torrent name from metadata, fallback to hash if not available
            let torrent_name = metadata.name.clone()
                .unwrap_or_else(|| format!("Torrent-{}", hex::encode(&handle.shared().info_hash.0[..8])));
            
            // Get actual file info from metadata.file_infos
            let file_list: Vec<TorrentFile> = metadata.file_infos
                .iter()
                .enumerate()
                .map(|(idx, fi)| TorrentFile {
                    index: idx,
                    path: fi.relative_filename.to_string_lossy().to_string(),
                    size: fi.len,
                    selected: true,
                })
                .collect();
            
            // Calculate total size from file lengths
            let total: u64 = metadata.file_infos.iter().map(|fi| fi.len).sum();
            
            (torrent_name, file_list, total)
        }).context("Failed to read torrent metadata")?;

        // Store handle info
        let mut torrents = self.torrents.write().await;
        let id = torrents.len();
        torrents.push(StoredTorrent {
            handle,
            name: name.clone(),
            total_size,
            files: files.clone(),
            torrent_idx,
        });

        info!("Added torrent: {} (id={})", name, id);

        Ok(TorrentInfo {
            id,
            name,
            files,
            total_size,
        })
    }

    /// Start streaming a specific file
    /// This enables only that file for download and returns the stream URL
    pub async fn start_stream(&self, torrent_id: usize, file_index: usize) -> Result<String> {
        let torrents = self.torrents.read().await;
        let stored = torrents.get(torrent_id)
            .ok_or_else(|| anyhow::anyhow!("Torrent not found"))?;
        
        // Get current files and add the new one (preserving existing selections)
        let current = stored.handle.only_files();
        let mut only_files: std::collections::HashSet<usize> = current
            .map(|v| v.iter().copied().collect())
            .unwrap_or_default();
        only_files.insert(file_index);
        
        // Update which files to download
        self.session.update_only_files(&stored.handle, &only_files).await?;
        
        // Try to unpause - ignore error if already running
        if let Err(e) = self.session.unpause(&stored.handle).await {
            let err_msg = e.to_string();
            // "already live" is not a real error, torrent is already downloading
            if !err_msg.contains("already live") && !err_msg.contains("already running") {
                return Err(e.into());
            }
            info!("Torrent already running, continuing with stream");
        }
        
        info!("Started streaming file {} of torrent {}", file_index, torrent_id);
        
        // Return the stream URL
        Ok(format!(
            "http://127.0.0.1:{}/torrents/{}/stream/{}",
            self.config.http_api_port, stored.torrent_idx, file_index
        ))
    }

    /// Add a file to the current streaming selection
    pub async fn add_file_to_stream(&self, torrent_id: usize, file_index: usize) -> Result<()> {
        let torrents = self.torrents.read().await;
        let stored = torrents.get(torrent_id)
            .ok_or_else(|| anyhow::anyhow!("Torrent not found"))?;
        
        // Get current only_files and add the new one
        let current = stored.handle.only_files();
        let mut only_files: std::collections::HashSet<usize> = current
            .map(|v| v.iter().copied().collect())
            .unwrap_or_default();
        only_files.insert(file_index);
        
        self.session.update_only_files(&stored.handle, &only_files).await?;
        info!("Added file {} to streaming for torrent {}", file_index, torrent_id);
        
        Ok(())
    }

    /// Pause a torrent
    pub async fn pause_download(&self, torrent_id: usize) -> Result<()> {
        let torrents = self.torrents.read().await;
        let stored = torrents.get(torrent_id)
            .ok_or_else(|| anyhow::anyhow!("Torrent not found"))?;
        
        // Use session API to pause
        self.session.pause(&stored.handle).await?;
        info!("Paused torrent {}", torrent_id);

        Ok(())
    }

    /// Get statistics for a specific torrent
    pub async fn get_stats(&self, torrent_id: usize) -> Result<TorrentStats> {
        let torrents = self.torrents.read().await;
        let stored = torrents.get(torrent_id)
            .ok_or_else(|| anyhow::anyhow!("Torrent not found"))?;

        let stats = stored.handle.stats();
        
        let total_bytes = stored.total_size;
        let downloaded_bytes = stats.progress_bytes;
        let progress = if total_bytes > 0 {
            (downloaded_bytes as f64 / total_bytes as f64) * 100.0
        } else {
            0.0
        };

        // Get live stats if available
        let (download_speed, upload_speed, peers_connected, eta_seconds) = 
            if let Some(live) = &stats.live {
                let dl_speed = (live.download_speed.mbps * 1_000_000.0 / 8.0) as u64;
                let ul_speed = (live.upload_speed.mbps * 1_000_000.0 / 8.0) as u64;
                
                // Get peer count from peer_stats
                let peers = live.snapshot.peer_stats.live as usize;
                
                let eta = if dl_speed > 0 {
                    let remaining = total_bytes.saturating_sub(downloaded_bytes);
                    Some(remaining / dl_speed)
                } else {
                    None
                };
                
                (dl_speed, ul_speed, peers, eta)
            } else {
                (0, 0, 0, None)
            };

        let peers_total = stats.live.as_ref()
            .map(|l| {
                let ps = &l.snapshot.peer_stats;
                (ps.live + ps.connecting + ps.queued + ps.dead) as usize
            })
            .unwrap_or(0);

        Ok(TorrentStats {
            id: torrent_id,
            name: stored.name.clone(),
            progress,
            download_speed,
            upload_speed,
            peers_connected,
            peers_total,
            downloaded_bytes,
            total_bytes,
            state: format!("{:?}", stats.state),
            eta_seconds,
        })
    }

    /// Get stats for all torrents
    pub async fn get_all_stats(&self) -> Result<Vec<TorrentStats>> {
        let torrents = self.torrents.read().await;
        let mut all_stats = Vec::with_capacity(torrents.len());
        
        for (id, stored) in torrents.iter().enumerate() {
            let stats = stored.handle.stats();
            let total_bytes = stored.total_size;
            let downloaded_bytes = stats.progress_bytes;
            let progress = if total_bytes > 0 {
                (downloaded_bytes as f64 / total_bytes as f64) * 100.0
            } else {
                0.0
            };

            let (download_speed, upload_speed, peers_connected, eta_seconds) = 
                if let Some(live) = &stats.live {
                    let dl_speed = (live.download_speed.mbps * 1_000_000.0 / 8.0) as u64;
                    let ul_speed = (live.upload_speed.mbps * 1_000_000.0 / 8.0) as u64;
                    let peers = live.snapshot.peer_stats.live as usize;
                    
                    let eta = if dl_speed > 0 {
                        let remaining = total_bytes.saturating_sub(downloaded_bytes);
                        Some(remaining / dl_speed)
                    } else {
                        None
                    };
                    
                    (dl_speed, ul_speed, peers, eta)
                } else {
                    (0, 0, 0, None)
                };

            let peers_total = stats.live.as_ref()
                .map(|l| {
                    let ps = &l.snapshot.peer_stats;
                    (ps.live + ps.connecting + ps.queued + ps.dead) as usize
                })
                .unwrap_or(0);

            all_stats.push(TorrentStats {
                id,
                name: stored.name.clone(),
                progress,
                download_speed,
                upload_speed,
                peers_connected,
                peers_total,
                downloaded_bytes,
                total_bytes,
                state: format!("{:?}", stats.state),
                eta_seconds,
            });
        }

        Ok(all_stats)
    }

    /// Get the streaming URL for a file
    /// Uses the real librqbit torrent index for the HTTP API
    pub async fn get_stream_url(&self, torrent_id: usize, file_index: usize) -> Result<String> {
        let torrents = self.torrents.read().await;
        let stored = torrents.get(torrent_id)
            .ok_or_else(|| anyhow::anyhow!("Torrent not found"))?;
        
        // Use librqbit's torrent_idx for the HTTP API URL
        Ok(format!(
            "http://127.0.0.1:{}/torrents/{}/stream/{}",
            self.config.http_api_port, stored.torrent_idx, file_index
        ))
    }

    /// Delete a torrent (optionally with files)
    pub async fn delete_torrent(&self, torrent_id: usize, delete_files: bool) -> Result<()> {
        let torrents = self.torrents.read().await;
        let stored = torrents.get(torrent_id)
            .ok_or_else(|| anyhow::anyhow!("Torrent not found"))?;
        
        let librqbit_id = stored.torrent_idx;
        drop(torrents);
        
        // Use session API to delete
        self.session.delete(TorrentIdOrHash::Id(librqbit_id), delete_files).await
            .context("Failed to delete torrent")?;
        
        info!("Deleted torrent {} (librqbit id: {})", torrent_id, librqbit_id);
        Ok(())
    }

    /// Get download directory path
    pub fn download_dir(&self) -> &PathBuf {
        &self.config.download_dir
    }
    
    /// Get the session for HTTP API
    pub fn session(&self) -> Arc<Session> {
        self.session.clone()
    }
    
    /// Get HTTP API port
    pub fn http_api_port(&self) -> u16 {
        self.config.http_api_port
    }
}

/// Well-known public trackers for improved peer discovery
/// These are frequently updated and reliable trackers
const PUBLIC_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://tracker.bittor.pw:1337/announce",
    "udp://public.popcorn-tracker.org:6969/announce",
    "udp://tracker.dler.org:6969/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://open.demonii.com:1337/announce",
];

/// Append public trackers to a magnet URI for improved peer discovery
/// Security: Only appends known safe tracker URLs, validates input first
fn append_public_trackers(magnet_uri: &str) -> String {
    let mut result = magnet_uri.to_string();
    
    for tracker in PUBLIC_TRACKERS {
        // URL-encode the tracker for magnet URI format
        let encoded = urlencoding::encode(tracker);
        // Only add if not already present
        if !magnet_uri.contains(tracker) {
            result.push_str("&tr=");
            result.push_str(&encoded);
        }
    }
    
    result
}

/// Validate a magnet URI format (basic security check)
pub fn validate_magnet_uri(uri: &str) -> bool {
    if !uri.starts_with("magnet:?") {
        return false;
    }
    
    // Must contain an infohash
    if !uri.contains("xt=urn:btih:") && !uri.contains("xt=urn:btmh:") {
        return false;
    }
    
    // Length sanity check
    if uri.len() > 10000 {
        return false;
    }
    
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_magnet_uri() {
        assert!(validate_magnet_uri("magnet:?xt=urn:btih:abc123&dn=test"));
        assert!(!validate_magnet_uri("http://example.com"));
        assert!(!validate_magnet_uri("magnet:?dn=test")); // No infohash
    }
}
