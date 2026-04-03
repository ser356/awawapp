//! CLI binary for awawapp - run without the Tauri UI
//!
//! Usage:
//!   awaw add <magnet>              Add a magnet link
//!   awaw play <magnet> [--vlc]     Add and stream in one step
//!   awaw stream <id> <file_idx>    Start streaming a file
//!   awaw stats [id] [-w]           Show statistics
//!   awaw history                   View history
//!
//! For full help: awaw --help

use awawapp_lib::cli::{Cli, CliApp};
use clap::Parser;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("info")
    } else {
        EnvFilter::new("warn")
    };

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(false).without_time())
        .with(filter)
        .init();

    // Create and run the CLI app
    match CliApp::new(cli.port).await {
        Ok(app) => {
            if let Err(e) = app.run(cli.command).await {
                eprintln!("\x1b[31mError:\x1b[0m {}", e);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("\x1b[31mFailed to initialize:\x1b[0m {}", e);
            std::process::exit(1);
        }
    }
}
