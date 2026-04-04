//! HLS transcoding server for browser-compatible streaming.
//!
//! Converts MKV/AVI/etc. to HLS (HTTP Live Streaming) via ffmpeg.
//! WKWebView (Tauri on macOS) natively supports HLS playback.
//!
//! Architecture:
//!   - `start_hls_transcode()` is called from a Tauri command — it starts
//!     ffmpeg, waits for the first segment, and returns the playlist URL.
//!   - `seek_hls_transcode()` restarts ffmpeg from a specific time position.
//!   - The HTTP server at port 3031 only serves HLS files (playlist + segments).

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};

/// Info about an active transcode session.
struct TranscodeSession {
    child: Child,
    start_time_secs: f64,
}

/// Shared transcode state — accessible from both the HTTP server and Tauri commands.
pub struct TranscodeState {
    /// Base URL for the torrent stream (librqbit HTTP API).
    pub stream_base_url: String,
    /// Base directory for HLS segment output.
    hls_base_dir: PathBuf,
    /// Port the HLS server listens on.
    port: u16,
    /// Active ffmpeg processes keyed by "torrent_id/file_index".
    active: Mutex<HashMap<String, TranscodeSession>>,
}

impl TranscodeState {
    pub fn new(port: u16, stream_base_url: String) -> Arc<Self> {
        let hls_base_dir = std::env::temp_dir().join("awawapp_hls");
        Arc::new(Self {
            stream_base_url,
            hls_base_dir,
            port,
            active: Mutex::new(HashMap::new()),
        })
    }
}

/// Start the HLS file server (only serves files, no transcoding logic).
pub async fn start_hls_server(state: Arc<TranscodeState>) -> anyhow::Result<()> {
    // Ensure base dir exists.
    tokio::fs::create_dir_all(&state.hls_base_dir).await?;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/hls/:torrent_id/:file_index/:filename", get(serve_hls_file))
        .route("/health", get(|| async { "ok" }))
        .layer(cors)
        .with_state(state.clone());

    let addr = format!("127.0.0.1:{}", state.port);
    let listener = TcpListener::bind(&addr).await?;
    info!("HLS server running on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

// ── Public API: start HLS transcode ─────────────────────────────────────

/// Probe the video codec using ffprobe.
async fn probe_video_codec(source_url: &str) -> Option<String> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=codec_name",
            "-of", "csv=p=0",
            "-probesize", "50000000",
            "-analyzeduration", "60000000",
            source_url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        warn!("ffprobe failed: {}", String::from_utf8_lossy(&output.stderr));
        return None;
    }
    let codec = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if codec.is_empty() { None } else { Some(codec) }
}

/// Probe the video duration using ffprobe.
async fn probe_video_duration(source_url: &str) -> Option<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "csv=p=0",
            "-probesize", "50000000",
            "-analyzeduration", "60000000",
            source_url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        warn!("ffprobe duration failed: {}", String::from_utf8_lossy(&output.stderr));
        return None;
    }
    let duration_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    duration_str.parse::<f64>().ok()
}

fn hls_dir(base: &std::path::Path, torrent_id: u32, file_index: u32) -> PathBuf {
    base.join(format!("{}_{}", torrent_id, file_index))
}

/// HLS transcode result with URL and duration.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HlsResult {
    pub url: String,
    pub duration_secs: Option<f64>,
}

/// Start HLS transcoding for a torrent file.
///
/// Returns the playlist URL and duration once the first segment is ready, e.g.
/// `http://127.0.0.1:3031/hls/0/0/playlist.m3u8`.
///
/// Called from the Tauri `get_transcode_url` command — NOT from an HTTP handler.
pub async fn start_hls_transcode(
    state: &Arc<TranscodeState>,
    torrent_id: u32,
    file_index: u32,
) -> anyhow::Result<HlsResult> {
    let key = format!("{}/{}", torrent_id, file_index);
    let source_url = format!(
        "{}/torrents/{}/stream/{}",
        state.stream_base_url, torrent_id, file_index
    );

    info!("=== HLS TRANSCODE START ===");
    info!("Source: {}", source_url);

    // If a transcode is already running for this stream AND the output dir
    // still has a playlist, just return the existing URL (idempotent).
    {
        let active = state.active.lock().await;
        if active.contains_key(&key) {
            let out_dir = hls_dir(&state.hls_base_dir, torrent_id, file_index);
            if out_dir.join("playlist.m3u8").exists() {
                info!("Transcode already running for {}, reusing", key);
                // Try to get duration
                let duration = probe_video_duration(&source_url).await;
                return Ok(HlsResult {
                    url: format!(
                        "http://127.0.0.1:{}/hls/{}/{}/playlist.m3u8",
                        state.port, torrent_id, file_index
                    ),
                    duration_secs: duration,
                });
            }
        }
    }

    // Kill any previous (stale) transcode for this stream.
    {
        let mut map = state.active.lock().await;
        if let Some(mut session) = map.remove(&key) {
            let _ = session.child.kill().await;
            info!("Killed previous ffmpeg for {}", key);
        }
    }

    // Prepare output directory (clean slate).
    let out_dir = hls_dir(&state.hls_base_dir, torrent_id, file_index);
    let _ = tokio::fs::remove_dir_all(&out_dir).await;
    tokio::fs::create_dir_all(&out_dir).await?;

    // ── source check ────────────────────────────────────────────────────
    // Verify the stream is accessible and has data before starting ffmpeg
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    
    // First, check if stream is accessible
    let resp = client.head(&source_url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Source stream not accessible: {}", resp.status());
    }
    info!("Source HEAD OK: status={}, len={:?}",
        resp.status(), resp.headers().get("content-length"));
    
    // Try to read some actual bytes to ensure data is flowing
    info!("Verifying stream has data...");
    let data_check = client
        .get(&source_url)
        .header("Range", "bytes=0-65535") // Request first 64KB
        .send()
        .await;
    
    match data_check {
        Ok(resp) if resp.status().is_success() || resp.status() == reqwest::StatusCode::PARTIAL_CONTENT => {
            let bytes = resp.bytes().await.unwrap_or_default();
            if bytes.len() < 1024 {
                warn!("Stream returned only {} bytes, torrent may be slow", bytes.len());
            } else {
                info!("Stream data verified: {} bytes readable", bytes.len());
            }
        }
        Ok(resp) => {
            warn!("Stream data check returned {}, proceeding anyway", resp.status());
        }
        Err(e) => {
            warn!("Could not verify stream data: {}, proceeding anyway", e);
        }
    }

    // ── codec detection ─────────────────────────────────────────────────
    let detected = probe_video_codec(&source_url).await;
    info!("Detected video codec: {:?}", detected);

    let vcodec = match detected.as_deref() {
        Some("h264") => "copy".to_string(),
        Some(other) => {
            info!("Codec '{}' not browser-compatible, re-encoding to H.264", other);
            "libx264".to_string()
        }
        None => {
            warn!("Could not detect codec, defaulting to copy");
            "copy".to_string()
        }
    };
    let acodec = "aac";
    info!("Using vcodec={}, acodec={}", vcodec, acodec);

    // ── build ffmpeg command ────────────────────────────────────────────
    let playlist_path = out_dir.join("playlist.m3u8");
    let segment_pattern = out_dir.join("seg%04d.ts");

    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(), "info".into(), // More verbose for debugging
        "-fflags".into(), "+genpts+discardcorrupt".into(), // Handle incomplete data
        "-err_detect".into(), "ignore_err".into(), // Ignore minor errors
        "-seekable".into(), "0".into(),
        "-probesize".into(), "100000000".into(), // 100MB probe for slow streams
        "-analyzeduration".into(), "180000000".into(), // 180 seconds analyze
        "-reconnect".into(), "1".into(),
        "-reconnect_streamed".into(), "1".into(),
        "-reconnect_delay_max".into(), "60".into(), // Longer reconnect delay
        "-reconnect_on_network_error".into(), "1".into(),
        "-reconnect_on_http_error".into(), "5xx".into(),
        "-timeout".into(), "180000000".into(), // 180 second timeout
        "-rw_timeout".into(), "60000000".into(), // 60 second read/write timeout
        "-i".into(), source_url.clone(),
        "-map".into(), "0:v:0?".into(),
        "-map".into(), "0:a:0?".into(),
        "-c:v".into(), vcodec.clone(),
    ];

    if vcodec == "libx264" {
        args.extend([
            "-preset".into(), "ultrafast".into(),
            "-crf".into(), "23".into(),
            "-tune".into(), "film".into(),
            "-profile:v".into(), "high".into(),
            "-level".into(), "4.1".into(),
            "-pix_fmt".into(), "yuv420p".into(),
        ]);
    }

    args.extend([
        "-c:a".into(), acodec.into(),
        "-b:a".into(), "192k".into(),
        "-f".into(), "hls".into(),
        "-hls_time".into(), "4".into(),
        "-hls_list_size".into(), "0".into(),
        // EVENT type: player knows segments won't be removed and can seek
        // within available content.  Duration grows as new segments appear.
        "-hls_playlist_type".into(), "event".into(),
        "-hls_flags".into(), "append_list".into(),
        "-hls_segment_type".into(), "mpegts".into(),
        "-hls_segment_filename".into(), segment_pattern.to_string_lossy().into(),
        playlist_path.to_string_lossy().into(),
    ]);

    let mut cmd = Command::new("ffmpeg");
    cmd.args(&args);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let mut child = cmd.spawn()?;

    // Log stderr in background.
    if let Some(stderr) = child.stderr.take() {
        let k = key.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.is_empty() {
                    warn!("ffmpeg [{}]: {}", k, line);
                }
            }
        });
    }

    // Store the session.
    state.active.lock().await.insert(key.clone(), TranscodeSession {
        child,
        start_time_secs: 0.0,
    });

    // ── wait for the first segment ──────────────────────────────────────
    // Increased timeout for slow torrents - ffmpeg needs continuous data
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(180);
    let mut last_log = tokio::time::Instant::now();
    let ready = loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        // Check if ffmpeg is still running
        {
            let mut map = state.active.lock().await;
            if let Some(session) = map.get_mut(&key) {
                match session.child.try_wait() {
                    Ok(Some(status)) => {
                        error!("ffmpeg exited prematurely with status: {}", status);
                        map.remove(&key);
                        anyhow::bail!("ffmpeg failed to start transcoding. The torrent stream may not have enough data.");
                    }
                    Ok(None) => {} // Still running, good
                    Err(e) => {
                        error!("Error checking ffmpeg status: {}", e);
                    }
                }
            }
        }
        
        // Log progress every 10 seconds
        if last_log.elapsed() >= std::time::Duration::from_secs(10) {
            let elapsed = (tokio::time::Instant::now() - (deadline - tokio::time::Duration::from_secs(180))).as_secs();
            info!("Waiting for HLS segment... ({}s elapsed, playlist exists: {})", 
                  elapsed, playlist_path.exists());
            last_log = tokio::time::Instant::now();
        }
        
        if playlist_path.exists() {
            // Check for at least one .ts segment
            if let Ok(mut entries) = tokio::fs::read_dir(&out_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if entry.path().extension().map_or(false, |e| e == "ts") {
                        info!("First HLS segment ready: {:?}", entry.path());
                        break;
                    }
                }
            }
            // Playlist exists — good enough even if the first .ts is still
            // being written.  The HLS player will retry segments it can't load.
            break true;
        }
        if tokio::time::Instant::now() >= deadline {
            break false;
        }
    };

    if !ready {
        // Kill the stalled ffmpeg.
        let mut map = state.active.lock().await;
        if let Some(mut session) = map.remove(&key) {
            let _ = session.child.kill().await;
        }
        anyhow::bail!("Timeout waiting for first HLS segment after 180s. The torrent may be too slow or have no seeders.");
    }

    // Probe duration
    let duration = probe_video_duration(&source_url).await;
    info!("Detected duration: {:?}s", duration);

    let url = format!(
        "http://127.0.0.1:{}/hls/{}/{}/playlist.m3u8",
        state.port, torrent_id, file_index
    );
    info!("HLS ready: {}", url);
    Ok(HlsResult { url, duration_secs: duration })
}

/// Seek HLS transcoding to a specific time position.
///
/// Stops the current ffmpeg process and restarts it with -ss to seek to the
/// specified time. Returns the new playlist URL and duration.
pub async fn seek_hls_transcode(
    state: &Arc<TranscodeState>,
    torrent_id: u32,
    file_index: u32,
    seek_time_secs: f64,
) -> anyhow::Result<HlsResult> {
    let key = format!("{}/{}", torrent_id, file_index);
    let source_url = format!(
        "{}/torrents/{}/stream/{}",
        state.stream_base_url, torrent_id, file_index
    );
    
    info!("=== HLS SEEK to {:.2}s ===", seek_time_secs);
    
    // Kill the current ffmpeg process
    {
        let mut map = state.active.lock().await;
        if let Some(mut session) = map.remove(&key) {
            let _ = session.child.kill().await;
            info!("Killed ffmpeg for seek: {}", key);
        }
    }
    
    // Clean output directory
    let out_dir = hls_dir(&state.hls_base_dir, torrent_id, file_index);
    let _ = tokio::fs::remove_dir_all(&out_dir).await;
    tokio::fs::create_dir_all(&out_dir).await?;
    
    // Detect codec
    let detected = probe_video_codec(&source_url).await;
    let vcodec = match detected.as_deref() {
        Some("h264") => "copy".to_string(),
        Some(_) => "libx264".to_string(),
        None => "copy".to_string(),
    };
    let acodec = "aac";
    
    // Build ffmpeg command with -ss for seeking
    let playlist_path = out_dir.join("playlist.m3u8");
    let segment_pattern = out_dir.join("seg%04d.ts");
    
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(), "warning".into(),
        "-ss".into(), format!("{:.3}", seek_time_secs),
        "-seekable".into(), "0".into(),
        "-probesize".into(), "50000000".into(),
        "-analyzeduration".into(), "120000000".into(),
        "-reconnect".into(), "1".into(),
        "-reconnect_streamed".into(), "1".into(),
        "-reconnect_delay_max".into(), "30".into(),
        "-timeout".into(), "120000000".into(),
        "-i".into(), source_url.clone(),
        "-map".into(), "0:v:0?".into(),
        "-map".into(), "0:a:0?".into(),
        "-c:v".into(), vcodec.clone(),
    ];

    if vcodec == "libx264" {
        args.extend([
            "-preset".into(), "ultrafast".into(),
            "-crf".into(), "23".into(),
            "-tune".into(), "film".into(),
            "-profile:v".into(), "high".into(),
            "-level".into(), "4.1".into(),
            "-pix_fmt".into(), "yuv420p".into(),
        ]);
    }

    args.extend([
        "-c:a".into(), acodec.into(),
        "-b:a".into(), "192k".into(),
        "-f".into(), "hls".into(),
        "-hls_time".into(), "4".into(),
        "-hls_list_size".into(), "0".into(),
        "-hls_playlist_type".into(), "event".into(),
        "-hls_flags".into(), "append_list".into(),
        "-hls_segment_type".into(), "mpegts".into(),
        "-hls_segment_filename".into(), segment_pattern.to_string_lossy().into(),
        playlist_path.to_string_lossy().into(),
    ]);

    let mut cmd = Command::new("ffmpeg");
    cmd.args(&args);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let mut child = cmd.spawn()?;

    // Log stderr
    if let Some(stderr) = child.stderr.take() {
        let k = key.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.is_empty() {
                    warn!("ffmpeg [{}]: {}", k, line);
                }
            }
        });
    }

    // Store session with seek time
    state.active.lock().await.insert(key.clone(), TranscodeSession {
        child,
        start_time_secs: seek_time_secs,
    });

    // Wait for first segment
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(60);
    let ready = loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
        if playlist_path.exists() {
            if let Ok(mut entries) = tokio::fs::read_dir(&out_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if entry.path().extension().map_or(false, |e| e == "ts") {
                        break;
                    }
                }
            }
            break true;
        }
        if tokio::time::Instant::now() >= deadline {
            break false;
        }
    };

    if !ready {
        let mut map = state.active.lock().await;
        if let Some(mut session) = map.remove(&key) {
            let _ = session.child.kill().await;
        }
        anyhow::bail!("Timeout waiting for HLS segment after seek");
    }

    // Probe duration
    let duration = probe_video_duration(&source_url).await;

    let url = format!(
        "http://127.0.0.1:{}/hls/{}/{}/playlist.m3u8?t={}",
        state.port, torrent_id, file_index, seek_time_secs as u64
    );
    info!("HLS ready after seek: {}", url);
    Ok(HlsResult { url, duration_secs: duration })
}

// ── Serve HLS files ─────────────────────────────────────────────────────

async fn serve_hls_file(
    State(state): State<Arc<TranscodeState>>,
    Path((torrent_id, file_index, filename)): Path<(u32, u32, String)>,
) -> Response {
    // Sanitise filename.
    let safe_name = std::path::Path::new(&filename)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if safe_name.is_empty() || safe_name.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let file_path = hls_dir(&state.hls_base_dir, torrent_id, file_index).join(&safe_name);
    if !file_path.exists() {
        return StatusCode::NOT_FOUND.into_response();
    }

    let content_type = if safe_name.ends_with(".m3u8") {
        "application/vnd.apple.mpegurl"
    } else if safe_name.ends_with(".ts") {
        "video/mp2t"
    } else {
        "application/octet-stream"
    };

    match tokio::fs::read(&file_path).await {
        Ok(bytes) => {
            // For playlists, verify the content is complete (ends with a
            // newline after the last segment entry).  ffmpeg writes the file
            // non-atomically, so we might catch a partial write.
            if safe_name.ends_with(".m3u8") {
                let text = String::from_utf8_lossy(&bytes);
                if !text.ends_with('\n') || !text.contains("#EXTINF:") {
                    // Playlist is still being written — ask client to retry.
                    return Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .header(header::RETRY_AFTER, "1")
                        .body(Body::empty())
                        .unwrap();
                }
            }
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CACHE_CONTROL, "no-cache, no-store")
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(e) => {
            error!("Failed to read HLS file {:?}: {}", file_path, e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
