use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use sms_types::events::{Event, EventKind};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use tracing::log::{debug, error, warn};
use uuid::Uuid;

pub type WebSocketConnection = (axum::extract::ws::WebSocket, Option<Vec<EventKind>>);
type StoredConnection = (UnboundedSender<axum::extract::ws::Utf8Bytes>, u8); // sender + event mask

#[derive(Clone)]
pub struct WebSocketManager {
    connections: Arc<DashMap<String, StoredConnection>>,
}
impl WebSocketManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
        }
    }

    pub fn broadcast(&self, event: Event) -> usize {
        let message = match serde_json::to_string(&event) {
            Ok(msg) => axum::extract::ws::Utf8Bytes::from(msg),
            Err(e) => {
                error!("Couldn't broadcast event '{event:?}' due to serialization error: {e}");
                return 0;
            }
        };

        let event_bit = EventKind::from(&event).to_bit();
        let mut successful_sends = 0;

        self.connections.retain(|_id, (sender, event_mask)| {
            if *event_mask & event_bit == 0 {
                return true;
            }

            if sender.send(message.clone()).is_ok() {
                successful_sends += 1;
                true
            } else {
                false
            }
        });

        successful_sends
    }

    pub fn add_connection(
        &self,
        tx: UnboundedSender<axum::extract::ws::Utf8Bytes>,
        events: Option<Vec<EventKind>>,
    ) -> String {
        let event_mask = match events {
            Some(event_types) => EventKind::events_to_mask(&event_types),
            None => EventKind::all_bits(),
        };

        loop {
            let id = Uuid::new_v4().to_string();
            if !self.connections.contains_key(&id) {
                self.connections.insert(id.clone(), (tx, event_mask));
                return id;
            }
        }
    }

    pub fn remove_connection(&self, id: &str) {
        self.connections.remove(id);
    }
}

// Called after the connection is upgraded.
pub async fn handle_websocket(connection: WebSocketConnection, manager: WebSocketManager) {
    let (mut sender, mut receiver) = connection.0.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<axum::extract::ws::Utf8Bytes>();

    // Add connection.
    let connection_id = manager.add_connection(tx, connection.1);
    debug!("WebSocket connection established: {connection_id}");

    // Writer task.
    let connection_id_for_tx = connection_id.clone();
    let (ping_tx, mut ping_rx) = mpsc::unbounded_channel();
    let tx_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    let Some(msg) = msg else { return }; // Channel closed
                    if sender.send(axum::extract::ws::Message::Text(msg)).await.is_err() {
                        return;
                    }
                },
                ping_data = ping_rx.recv() => {
                    let Some(data) = ping_data else { return }; // Channel closed
                    if sender.send(axum::extract::ws::Message::Pong(data)).await.is_err() {
                        return;
                    }
                }
            }
        }
    });

    // Reader.
    let rx_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(axum::extract::ws::Message::Text(text)) => {
                    debug!("Received WebSocket message from {connection_id}: {text:?}")
                }
                Ok(axum::extract::ws::Message::Ping(ping)) => {
                    if ping_tx.send(ping).is_err() {
                        break;
                    }
                }
                Ok(axum::extract::ws::Message::Close(_)) => {
                    debug!("WebSocket connection closed: {connection_id}");
                    break;
                }
                Err(e) => {
                    // Check for common disconnection errors.
                    let is_expected_disconnect = std::error::Error::source(&e)
                        .and_then(|e| e.downcast_ref::<std::io::Error>())
                        .is_some_and(|io_err| {
                            matches!(
                                io_err.kind(),
                                std::io::ErrorKind::UnexpectedEof
                                    | std::io::ErrorKind::ConnectionReset
                            )
                        });

                    if is_expected_disconnect {
                        debug!("WebSocket connection closed: {connection_id}");
                    } else {
                        warn!("WebSocket error for {connection_id}: {e}");
                    }
                    break;
                }
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = tx_task => {},
        _ = rx_task => {},
    }

    // Remove connection after either task finishes.
    manager.remove_connection(&connection_id_for_tx);
    debug!("WebSocket connection cleaned up: {connection_id_for_tx}");
}
