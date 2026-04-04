//! CLI module for awawapp - torrent streaming from the command line.
//!
//! This module provides a command-line interface that mirrors the functionality
//! of the GUI application, allowing users to stream torrents without a UI.

use crate::database::Database;
use crate::torrent_engine::{EngineConfig, TorrentEngine, TorrentStats};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use console::{style, Term};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// awawapp CLI - Stream torrents from the command line
#[derive(Parser, Debug)]
#[command(name = "awaw")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// HTTP API port for streaming (default: 3030)
    #[arg(short, long, global = true, default_value = "3030")]
    pub port: u16,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Add a magnet link and fetch its metadata
    Add {
        /// Magnet URI to add
        #[arg(required_unless_present = "file")]
        magnet: Option<String>,

        /// Path to .torrent file
        #[arg(short, long)]
        file: Option<PathBuf>,
    },

    /// List files in an added torrent
    Files {
        /// Torrent ID (from add command)
        torrent_id: usize,
    },

    /// Start streaming a file and get the stream URL
    Stream {
        /// Torrent ID
        torrent_id: usize,

        /// File index within the torrent
        file_index: usize,

        /// Automatically open in VLC
        #[arg(long)]
        vlc: bool,

        /// Copy URL to clipboard
        #[arg(long)]
        copy: bool,
    },

    /// Show real-time statistics for a torrent
    Stats {
        /// Torrent ID (omit to show all)
        torrent_id: Option<usize>,

        /// Watch mode - continuously update stats
        #[arg(short, long)]
        watch: bool,
    },

    /// Pause a torrent
    Pause {
        /// Torrent ID to pause
        torrent_id: usize,
    },

    /// Delete a torrent from the session
    Delete {
        /// Torrent ID to delete
        torrent_id: usize,

        /// Also delete downloaded files
        #[arg(long)]
        files: bool,
    },

    /// View magnet link history
    History {
        /// Maximum entries to show
        #[arg(short, long, default_value = "20")]
        limit: u32,

        /// Search query
        #[arg(short, long)]
        search: Option<String>,
    },

    /// Interactive mode - add magnet and stream in one step
    Play {
        /// Magnet URI to play
        magnet: String,

        /// File index to play (default: largest file)
        #[arg(short, long)]
        file: Option<usize>,

        /// Open in VLC automatically
        #[arg(long)]
        vlc: bool,
    },
}

/// CLI application state
pub struct CliApp {
    engine: Arc<TorrentEngine>,
    database: Arc<Database>,
    #[allow(dead_code)]
    config: EngineConfig,
}

impl CliApp {
    /// Initialize the CLI application with the given configuration
    pub async fn new(port: u16) -> Result<Self> {
        // Use cache dir for librqbit (required path, but nothing is written - all goes to RAM)
        let config = EngineConfig {
            download_dir: dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("awawapp"),
            http_api_port: port,
            ..Default::default()
        };

        // Initialize database
        let db_path = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("awawapp")
            .join("history.db");
        
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let database = Arc::new(Database::new(&db_path)?);
        let engine = Arc::new(TorrentEngine::new(config.clone()).await?);

        Ok(Self {
            engine,
            database,
            config,
        })
    }

    /// Run a CLI command
    pub async fn run(&self, command: Commands) -> Result<()> {
        match command {
            Commands::Add { magnet, file } => self.cmd_add(magnet, file).await,
            Commands::Files { torrent_id } => self.cmd_files(torrent_id).await,
            Commands::Stream { torrent_id, file_index, vlc, copy } => {
                self.cmd_stream(torrent_id, file_index, vlc, copy).await
            }
            Commands::Stats { torrent_id, watch } => self.cmd_stats(torrent_id, watch).await,
            Commands::Pause { torrent_id } => self.cmd_pause(torrent_id).await,
            Commands::Delete { torrent_id, files } => self.cmd_delete(torrent_id, files).await,
            Commands::History { limit, search } => self.cmd_history(limit, search).await,
            Commands::Play { magnet, file, vlc } => self.cmd_play(magnet, file, vlc).await,
        }
    }

    /// Add a magnet link or torrent file
    async fn cmd_add(&self, magnet: Option<String>, file: Option<PathBuf>) -> Result<()> {
        let info = if let Some(magnet_uri) = magnet {
            println!("{} Adding magnet link...", style("⏳").cyan());
            
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .unwrap()
            );
            pb.set_message("Fetching metadata from peers...");
            pb.enable_steady_tick(Duration::from_millis(100));

            let result = self.engine.add_magnet(&magnet_uri, false).await;
            pb.finish_and_clear();

            match result {
                Ok(info) => {
                    // Save to history
                    let _ = self.database.add_magnet(&magnet_uri, &info.name);
                    info
                }
                Err(e) => {
                    println!("{} Failed to add magnet: {}", style("✗").red(), e);
                    return Err(e);
                }
            }
        } else if let Some(path) = file {
            println!("{} Adding torrent file: {}", style("⏳").cyan(), path.display());
            let bytes = std::fs::read(&path).context("Failed to read torrent file")?;
            self.engine.add_torrent_file(bytes).await?
        } else {
            anyhow::bail!("Either magnet or file must be provided");
        };

        // Display torrent info
        println!();
        println!("{} Torrent added successfully!", style("✓").green());
        println!();
        println!("  {} {}", style("ID:").bold(), info.id);
        println!("  {} {}", style("Name:").bold(), info.name);
        println!("  {} {}", style("Size:").bold(), format_size(info.total_size));
        println!("  {} {}", style("Files:").bold(), info.files.len());
        println!();
        
        // List files
        println!("{}", style("Files:").bold().underlined());
        for file in &info.files {
            println!(
                "  {} [{}] {} ({})",
                style(format!("{:>3}", file.index)).dim(),
                if file.selected { style("✓").green() } else { style(" ").dim() },
                file.path,
                format_size(file.size)
            );
        }
        println!();
        println!(
            "{} Use '{}' to start streaming",
            style("→").cyan(),
            style(format!("awaw stream {} <file_index>", info.id)).yellow()
        );

        Ok(())
    }

    /// List files in a torrent
    async fn cmd_files(&self, torrent_id: usize) -> Result<()> {
        let stats = self.engine.get_stats(torrent_id).await?;
        
        println!();
        println!("{} {} (ID: {})", style("📁").cyan(), style(&stats.name).bold(), torrent_id);
        println!("   Total size: {}", format_size(stats.total_bytes));
        println!();
        
        // Get file list from engine
        // Note: In a full implementation, we'd store file info
        println!("{} Use 'awaw add <magnet>' to see file list", style("ℹ").blue());
        
        Ok(())
    }

    /// Start streaming a file
    async fn cmd_stream(
        &self,
        torrent_id: usize,
        file_index: usize,
        open_vlc: bool,
        _copy: bool,
    ) -> Result<()> {
        println!("{} Starting stream...", style("⏳").cyan());
        
        let url = self.engine.start_stream(torrent_id, file_index).await?;
        
        println!();
        println!("{} Stream ready!", style("✓").green());
        println!();
        println!("  {} {}", style("Stream URL:").bold(), style(&url).cyan().underlined());
        println!();
        
        if open_vlc {
            println!("{} Opening in VLC...", style("→").cyan());
            open_in_vlc(&url)?;
        } else {
            println!("{}", style("Tips:").bold());
            println!("  • Open this URL in VLC: {}", style("File → Open Network Stream").dim());
            println!("  • Or run: {}", style(format!("open -a VLC \"{}\"", url)).yellow());
            println!("  • Use --vlc flag to open automatically");
        }

        Ok(())
    }

    /// Show torrent statistics
    async fn cmd_stats(&self, torrent_id: Option<usize>, watch: bool) -> Result<()> {
        let term = Term::stdout();
        
        loop {
            if watch {
                term.clear_screen()?;
            }

            if let Some(id) = torrent_id {
                let stats = self.engine.get_stats(id).await?;
                print_stats(&stats);
            } else {
                let all_stats = self.engine.get_all_stats().await?;
                if all_stats.is_empty() {
                    println!("{} No active torrents", style("ℹ").blue());
                } else {
                    for stats in &all_stats {
                        print_stats(stats);
                        println!();
                    }
                }
            }

            if !watch {
                break;
            }
            
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        Ok(())
    }

    /// Pause a torrent
    async fn cmd_pause(&self, torrent_id: usize) -> Result<()> {
        self.engine.pause_download(torrent_id).await?;
        println!("{} Torrent {} paused", style("⏸").yellow(), torrent_id);
        Ok(())
    }

    /// Delete a torrent
    async fn cmd_delete(&self, torrent_id: usize, delete_files: bool) -> Result<()> {
        self.engine.delete_torrent(torrent_id, delete_files).await?;
        println!(
            "{} Torrent {} deleted{}",
            style("🗑").red(),
            torrent_id,
            if delete_files { " (including files)" } else { "" }
        );
        Ok(())
    }

    /// Show history
    async fn cmd_history(&self, limit: u32, search: Option<String>) -> Result<()> {
        let history = if let Some(query) = search {
            self.database.search(&query)?
        } else {
            self.database.get_history(Some(limit))?
        };

        if history.is_empty() {
            println!("{} No history found", style("ℹ").blue());
            return Ok(());
        }

        println!();
        println!("{}", style("Recent Torrents:").bold().underlined());
        println!();

        for entry in &history {
            println!(
                "  {} {} {}",
                style(format!("[{}]", entry.id)).dim(),
                entry.name,
                style(format!("({})", entry.added_at)).dim()
            );
            println!("     {}", style(&entry.magnet_link[..entry.magnet_link.len().min(60)]).dim());
            if entry.magnet_link.len() > 60 {
                println!("     {}", style("...").dim());
            }
        }
        println!();

        Ok(())
    }

    /// Play mode - add and stream in one step
    async fn cmd_play(&self, magnet: String, file_index: Option<usize>, open_vlc: bool) -> Result<()> {
        // First, add the magnet
        println!("{} Adding magnet and fetching metadata...", style("⏳").cyan());
        
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap()
        );
        pb.set_message("Connecting to peers...");
        pb.enable_steady_tick(Duration::from_millis(100));

        let info = self.engine.add_magnet(&magnet, false).await?;
        
        // Save to history
        let _ = self.database.add_magnet(&magnet, &info.name);
        
        pb.finish_and_clear();
        
        println!("{} {} ({})", style("✓").green(), info.name, format_size(info.total_size));
        println!();

        // Determine which file to play
        let target_index = if let Some(idx) = file_index {
            idx
        } else {
            // Find largest file (usually the main video)
            info.files
                .iter()
                .max_by_key(|f| f.size)
                .map(|f| f.index)
                .unwrap_or(0)
        };

        // Show file being played
        if let Some(file) = info.files.iter().find(|f| f.index == target_index) {
            println!(
                "  {} {} ({})",
                style("▶").green(),
                file.path,
                format_size(file.size)
            );
        }
        println!();

        // Start streaming
        let url = self.engine.start_stream(info.id, target_index).await?;
        
        println!("{} {}", style("Stream URL:").bold(), style(&url).cyan().underlined());
        println!();

        if open_vlc {
            println!("{} Opening in VLC...", style("→").cyan());
            open_in_vlc(&url)?;
            
            // Keep running to maintain the stream
            println!();
            println!("{} Press Ctrl+C to stop streaming", style("ℹ").blue());
            
            // Wait indefinitely (or until Ctrl+C)
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                
                // Show periodic stats
                if let Ok(stats) = self.engine.get_stats(info.id).await {
                    print!(
                        "\r{} Progress: {:.1}% | ↓ {}/s | Peers: {}   ",
                        style("📊").cyan(),
                        stats.progress * 100.0,
                        format_size(stats.download_speed),
                        stats.peers_connected
                    );
                    std::io::Write::flush(&mut std::io::stdout())?;
                }
            }
        } else {
            println!("{} Copy this URL to VLC or any media player", style("→").cyan());
        }

        Ok(())
    }
}

/// Format bytes to human-readable size
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Print torrent statistics
fn print_stats(stats: &TorrentStats) {
    let progress_bar = create_progress_bar(stats.progress, 30);
    
    println!("{} {} (ID: {})", style("📊").cyan(), style(&stats.name).bold(), stats.id);
    println!("   {} {:.1}%", progress_bar, stats.progress * 100.0);
    println!(
        "   ↓ {}/s  ↑ {}/s  Peers: {}/{}",
        format_size(stats.download_speed),
        format_size(stats.upload_speed),
        stats.peers_connected,
        stats.peers_total
    );
    println!(
        "   Downloaded: {} / {}  State: {}",
        format_size(stats.downloaded_bytes),
        format_size(stats.total_bytes),
        style(&stats.state).cyan()
    );
    
    if let Some(eta) = stats.eta_seconds {
        println!("   ETA: {}", format_duration(eta));
    }
}

/// Create a text-based progress bar
fn create_progress_bar(progress: f64, width: usize) -> String {
    let filled = (progress * width as f64) as usize;
    let empty = width - filled;
    
    format!(
        "{}{}{}{}",
        style("[").dim(),
        style("█".repeat(filled)).green(),
        style("░".repeat(empty)).dim(),
        style("]").dim()
    )
}

/// Format seconds to human-readable duration
fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    
    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

/// Open a URL in VLC with network caching for better streaming
fn open_in_vlc(url: &str) -> Result<()> {
    use std::process::Command;
    
    #[cfg(target_os = "macos")]
    {
        // --network-caching=5000 gives VLC 5 seconds of buffer
        // This prevents pauses when pieces are still downloading
        Command::new("open")
            .args(["-a", "VLC", "--args", "--network-caching=5000", url])
            .spawn()
            .context("Failed to open VLC. Is it installed?")?;
    }
    
    #[cfg(target_os = "windows")]
    {
        // Try common VLC paths on Windows
        let vlc_paths = [
            r"C:\Program Files\VideoLAN\VLC\vlc.exe",
            r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
        ];
        
        let vlc_path = vlc_paths.iter().find(|p| std::path::Path::new(p).exists());
        
        if let Some(path) = vlc_path {
            Command::new(path)
                .args(["--network-caching=5000", url])
                .spawn()
                .context("Failed to open VLC")?;
        } else {
            anyhow::bail!("VLC not found. Please install VLC or add it to PATH.");
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        Command::new("vlc")
            .args(["--network-caching=5000", url])
            .spawn()
            .context("Failed to open VLC. Is it installed?")?;
    }
    
    Ok(())
}
