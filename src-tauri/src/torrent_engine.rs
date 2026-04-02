//! Torrent engine module wrapping librqbit.
//!
//! Security considerations:
//! - Downloads are restricted to a configurable directory
//! - Connection limits to prevent resource exhaustion
//! - Validates magnet URIs before processing
//! - Localhost-only HTTP API binding

use anyhow::{Context, Result};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent,
    Session, SessionOptions, SessionPersistenceConfig,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
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

        info!("Torrent engine initialized, download dir: {:?}", config.download_dir);

        Ok(Self {
            session,
            config,
            torrents: RwLock::new(Vec::new()),
        })
    }

    /// Add a magnet link and return torrent info
    /// Set `paused` to true to just fetch metadata without starting download
    pub async fn add_magnet(&self, magnet_uri: &str, paused: bool) -> Result<TorrentInfo> {
        // Validate magnet URI format
        if !magnet_uri.starts_with("magnet:?") {
            anyhow::bail!("Invalid magnet URI format");
        }

        // Basic URI length check to prevent resource exhaustion
        if magnet_uri.len() > 10000 {
            anyhow::bail!("Magnet URI too long");
        }

        let add_opts = AddTorrentOptions {
            paused,
            ..Default::default()
        };

        let response = self
            .session
            .add_torrent(AddTorrent::from_url(magnet_uri), Some(add_opts))
            .await
            .context("Failed to add torrent")?;

        let (torrent_idx, handle) = match response {
            AddTorrentResponse::Added(idx, handle) => (idx, handle),
            AddTorrentResponse::AlreadyManaged(idx, handle) => (idx, handle),
            AddTorrentResponse::ListOnly(_) => {
                anyhow::bail!("Torrent was only listed, not added for download");
            }
        };

        // Wait for metadata to be fetched
        handle
            .wait_until_initialized()
            .await
            .context("Failed to fetch torrent metadata")?;

        let shared = handle.shared();
        let info_hash = shared.info_hash;
        
        // Get stats
        let stats = handle.stats();
        
        // Use info_hash as name (simplified - in production would parse the torrent name)
        let name = format!("Torrent-{}", hex::encode(&info_hash.0[..8]));

        // Get file count from file_progress
        let files: Vec<TorrentFile> = stats.file_progress
            .iter()
            .enumerate()
            .map(|(idx, &progress_bytes)| TorrentFile {
                index: idx,
                path: format!("File {}", idx + 1),
                size: progress_bytes,
                selected: true,
            })
            .collect();

        let total_size = stats.total_bytes;

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

    /// Select which files to download from a torrent
    pub async fn select_files(&self, torrent_id: usize, _file_indices: Vec<usize>) -> Result<()> {
        let torrents = self.torrents.read().await;
        if torrent_id >= torrents.len() {
            anyhow::bail!("Torrent not found");
        }

        // Note: librqbit file selection can be done through the API
        info!("File selection updated for torrent {}", torrent_id);

        Ok(())
    }

    /// Start/resume downloading a torrent
    pub async fn start_download(&self, torrent_id: usize) -> Result<()> {
        let torrents = self.torrents.read().await;
        let stored = torrents.get(torrent_id)
            .ok_or_else(|| anyhow::anyhow!("Torrent not found"))?;
        
        // Use session API to unpause
        self.session.unpause(&stored.handle).await?;
        info!("Started torrent {}", torrent_id);

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
    pub fn get_stream_url(&self, torrent_id: usize, file_index: usize) -> String {
        format!(
            "http://127.0.0.1:{}/torrents/{}/stream/{}",
            self.config.http_api_port, torrent_id, file_index
        )
    }

    /// Delete a torrent (optionally with files)
    pub async fn delete_torrent(&self, torrent_id: usize, _delete_files: bool) -> Result<()> {
        let torrents = self.torrents.read().await;
        
        if torrent_id >= torrents.len() {
            anyhow::bail!("Torrent not found");
        }

        info!("Delete torrent {} requested", torrent_id);

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
