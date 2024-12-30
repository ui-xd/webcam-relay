use anyhow::Result;
use futures::{SinkExt, StreamExt};
use log::{error, info};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_tungstenite::accept_async;

const LISTEN_ADDR: &str = "127.0.0.1:8080";
const OUTPUT_UDP: &str = "udp://127.0.0.1:1234";

struct FFmpegProcess {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    info!("Starting webcam relay server on {}", LISTEN_ADDR);

    let listener = TcpListener::bind(LISTEN_ADDR).await?;
    
    while let Ok((stream, addr)) = listener.accept().await {
        info!("New connection from {}", addr);
        tokio::spawn(handle_connection(stream));
    }

    Ok(())
}

async fn handle_connection(stream: TcpStream) -> Result<()> {
    let ws_stream = accept_async(stream).await?;
    info!("WebSocket connection established");

    // Initialize FFmpeg process
    let ffmpeg = Command::new("ffmpeg")
        .args([
            "-i", "pipe:0",         // Read from stdin
            "-c:v", "copy",         // Copy video codec (no transcoding)
            "-c:a", "aac",          // Convert audio to AAC
            "-ar", "44100",         // Audio sample rate
            "-f", "mpegts",         // Output format
            OUTPUT_UDP,             // Output to UDP for OBS
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let stdin = ffmpeg.stdin.expect("Failed to get FFmpeg stdin");
    
    let ffmpeg = Arc::new(Mutex::new(FFmpegProcess {
        child: ffmpeg,
        stdin,
    }));

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    
    // Handle incoming WebSocket messages
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(msg) => {
                if msg.is_binary() {
                    let mut ffmpeg = ffmpeg.lock().await;
                    if let Err(e) = ffmpeg.stdin.write_all(&msg.into_data()) {
                        error!("Failed to write to FFmpeg: {}", e);
                        break;
                    }
                }
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
        }
    }

    // Cleanup
    let mut ffmpeg = ffmpeg.lock().await;
    let _ = ffmpeg.child.kill();
    info!("Connection closed");

    Ok(())
}