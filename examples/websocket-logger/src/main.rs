use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use log::{info, warn};
use std::env::var;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::broadcast;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};

#[cfg(not(unix))]
use tokio::signal::ctrl_c;

const FILE_BUFFER_SIZE: usize = 64 * 1024;

struct WebSocketLogger {
    url: String,
    log_file_path: String,
    reconnect_delay: Duration,
}
impl WebSocketLogger {
    pub fn new(url: String, log_file_path: String) -> Self {
        Self {
            url,
            log_file_path,
            reconnect_delay: Duration::from_secs(5),
        }
    }

    pub async fn start(&self, shutdown_rx: broadcast::Receiver<()>) -> Result<()> {
        info!("Starting WebSocket logger for: {}", self.url);
        info!("Logging to: {}", self.log_file_path);

        let mut shutdown_rx = shutdown_rx;
        loop {
            // Check for shutdown signal before attempting connection
            if let Ok(_) = shutdown_rx.try_recv() {
                info!("Shutdown signal received");
                break;
            }

            match self.connect_and_log(shutdown_rx.resubscribe()).await {
                Ok(_) => {
                    info!("WebSocket connection closed normally");
                    break;
                }
                Err(e) => {
                    warn!(
                        "WebSocket error: {}. Reconnecting in {:?}...",
                        e, self.reconnect_delay
                    );
                    tokio::select! {
                        _ = sleep(self.reconnect_delay) => {},
                        _ = shutdown_rx.recv() => {
                            info!("Shutdown signal received during reconnect delay");
                            break;
                        }
                    }
                }
            }
        }

        info!("WebSocket logger stopped");
        Ok(())
    }

    async fn connect_and_log(&self, mut shutdown_rx: broadcast::Receiver<()>) -> Result<()> {
        info!("Connecting to WebSocket...");
        let (ws_stream, _) = connect_async(&self.url).await?;
        info!("WebSocket connected successfully!");

        let (mut write, mut read) = ws_stream.split();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file_path)
            .await?;

        let mut writer = BufWriter::with_capacity(FILE_BUFFER_SIZE, file);

        loop {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(msg) => {
                            match msg? {
                                Message::Text(text) => {
                                    writer.write_all(text.as_bytes()).await?;
                                    writer.write_all(b"\n").await?;
                                },
                                Message::Close(frame) => {
                                    let close_info = if let Some(frame) = frame {
                                        format!("code: {}, reason: {}", frame.code, frame.reason)
                                    } else {
                                        "no close frame".to_string()
                                    };
                                    writer.flush().await?;
                                    info!("WebSocket closed: {}", close_info);
                                    return Ok(());
                                },
                                Message::Binary(data) => {
                                    writer.write_all(&data).await?;
                                    writer.write_all(b"\n").await?;
                                },
                                Message::Ping(_) | Message::Pong(_) => {
                                    continue;
                                }
                                _ => continue
                            }
                        }
                        None => {
                            // Stream ended
                            writer.flush().await?;
                            warn!("WebSocket stream ended unexpectedly");
                            return Ok(());
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received, closing WebSocket connection gracefully");
                    writer.flush().await?;

                    // Send close frame
                    if let Err(e) = write.send(Message::Close(None)).await {
                        warn!("Failed to send close frame: {}", e);
                    }

                    return Ok(());
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    // Signal handlers.
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut sigint =
                signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");
            let mut sigterm =
                signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
            let mut sigquit =
                signal(SignalKind::quit()).expect("Failed to register SIGQUIT handler");

            tokio::select! {
                _ = sigint.recv() => {
                    info!("Received SIGINT (CTRL+C) signal");
                },
                _ = sigterm.recv() => {
                    info!("Received SIGTERM (kill) signal");
                },
                _ = sigquit.recv() => {
                    info!("Received SIGQUIT signal");
                },
            }
        }

        #[cfg(not(unix))]
        {
            match ctrl_c().await {
                Ok(()) => {
                    info!("Received CTRL+C signal");
                }
                Err(err) => {
                    warn!("Unable to listen for shutdown signal: {}", err);
                    return;
                }
            }
        }

        let _ = shutdown_tx_clone.send(());
    });

    let logger = WebSocketLogger::new(
        var("WEBSOCKET_LOGGER_URL")
            .context("Missing required WEBSOCKET_LOGGER_URL environment variable!")?,
        var("WEBSOCKET_LOGGER_FILEPATH").unwrap_or("websocket_messages.log".to_string()),
    );

    logger.start(shutdown_rx).await
}
