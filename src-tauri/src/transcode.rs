//! HLS transcoding server for browser-compatible streaming.
//!
//! Uses ffmpeg to transcode MKV/AVI/etc to fragmented MP4 (fMP4) for HTML5 video.
//! The transcoder runs as a separate HTTP server that proxies the torrent stream
//! through ffmpeg.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio_util::io::ReaderStream;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};

/// Transcode server state
pub struct TranscodeState {
    /// Base URL for the torrent stream (librqbit HTTP API)
    pub stream_base_url: String,
}

/// Query parameters for transcoding
#[derive(Deserialize)]
pub struct TranscodeParams {
    /// Optional: force video codec (copy = no re-encode, libx264 = re-encode)
    #[serde(default)]
    pub vcodec: Option<String>,
    /// Optional: force audio codec (copy = no re-encode, aac = re-encode)
    #[serde(default)]
    pub acodec: Option<String>,
}

/// Start the transcode server on the given port
pub async fn start_transcode_server(
    port: u16,
    stream_base_url: String,
) -> anyhow::Result<()> {
    let state = Arc::new(TranscodeState { stream_base_url });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/transcode/:torrent_id/:file_index", get(transcode_stream))
        .route("/health", get(health_check))
        .layer(cors)
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    
    info!("Transcode server running on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Health check endpoint
async fn health_check() -> &'static str {
    "ok"
}

/// Transcode a torrent stream to browser-compatible MP4
async fn transcode_stream(
    State(state): State<Arc<TranscodeState>>,
    Path((torrent_id, file_index)): Path<(u32, u32)>,
    Query(params): Query<TranscodeParams>,
) -> Response {
    let source_url = format!(
        "{}/torrents/{}/stream/{}",
        state.stream_base_url, torrent_id, file_index
    );

    info!("=== TRANSCODE REQUEST ===");
    info!("Transcoding stream from: {}", source_url);
    info!("Torrent ID: {}, File Index: {}", torrent_id, file_index);

    // Check if source is accessible first
    let client = reqwest::Client::new();
    match client.head(&source_url).send().await {
        Ok(resp) => {
            info!("Source check: status={}, content-length={:?}", 
                resp.status(), 
                resp.headers().get("content-length"));
            if !resp.status().is_success() {
                error!("Source stream not accessible: {}", resp.status());
                return (
                    StatusCode::BAD_GATEWAY,
                    format!("Source stream error: {}", resp.status()),
                ).into_response();
            }
        }
        Err(e) => {
            error!("Failed to check source stream: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                format!("Cannot reach source stream: {}", e),
            ).into_response();
        }
    }

    // Determine codecs
    // For most BD Remux content (H.264/H.265), we try to copy video and re-encode audio to AAC
    // If video is HEVC/H.265, browser won't support it, so we'd need to re-encode
    let vcodec = params.vcodec.unwrap_or_else(|| "copy".to_string());
    let acodec = params.acodec.unwrap_or_else(|| "aac".to_string());

    // Build ffmpeg command
    // -i: input from URL
    // -c:v copy: copy video stream without re-encoding (fast)
    // -c:a aac: encode audio to AAC (browser compatible)
    // -movflags frag_keyframe+empty_moov+faststart: fragmented MP4 for streaming
    // -f mp4: output format
    // pipe:1: output to stdout
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-hide_banner",
        "-loglevel", "info",  // More verbose for debugging
        // Input options - be patient with slow streams
        "-reconnect", "1",
        "-reconnect_streamed", "1",
        "-reconnect_delay_max", "30",
        "-timeout", "60000000",  // 60 seconds in microseconds
        "-i", &source_url,
        // Map first video and audio streams
        "-map", "0:v:0?",  // First video stream (optional - don't fail if missing)
        "-map", "0:a:0?",  // First audio stream (optional)
        // Video codec - copy if possible (fast)
        "-c:v", &vcodec,
        // Audio codec - AAC for browser compatibility
        "-c:a", &acodec,
        "-b:a", "192k",
        // Output format options for streaming fragmented MP4
        "-movflags", "frag_keyframe+empty_moov+default_base_moof",
        "-f", "mp4",
        // Output to stdout
        "pipe:1"
    ]);

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to spawn ffmpeg: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to start transcoder: {}", e),
            ).into_response();
        }
    };

    // Log stderr in background
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.is_empty() {
                    warn!("ffmpeg: {}", line);
                }
            }
        });
    }

    // Stream stdout to response
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            error!("Failed to get ffmpeg stdout");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get transcoder output",
            ).into_response();
        }
    };

    let stream = ReaderStream::new(stdout);
    let body = Body::from_stream(stream);

    // Spawn a task to wait for the child and log exit status
    tokio::spawn(async move {
        match child.wait().await {
            Ok(status) => {
                if !status.success() {
                    warn!("ffmpeg exited with status: {}", status);
                } else {
                    info!("ffmpeg completed successfully");
                }
            }
            Err(e) => error!("Error waiting for ffmpeg: {}", e),
        }
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap()
}

/// Alternative: HLS streaming (generates .m3u8 playlist and .ts segments)
/// This would require more complex state management to track segments.
/// For now, fMP4 streaming is simpler and works well for most cases.
#[allow(dead_code)]
pub async fn start_hls_server(_port: u16) -> anyhow::Result<()> {
    // HLS would require:
    // 1. Generate segments on-demand
    // 2. Track which segments exist
    // 3. Serve .m3u8 playlist
    // 4. Clean up old segments
    // 
    // fMP4 streaming is simpler for our use case
    unimplemented!("HLS server not yet implemented")
}
