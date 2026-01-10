use futures::{SinkExt, StreamExt};
use sms_types::events::{Event, EventKind};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{mpsc, RwLock};
use tracing::log::{debug, error, warn};
use uuid::Uuid;

pub type WebSocketConnection = (axum::extract::ws::WebSocket, Option<Vec<EventKind>>);
type StoredConnection = (UnboundedSender<axum::extract::ws::Utf8Bytes>, u8); // sender + event mask

#[derive(Clone)]
pub struct WebSocketManager {
    connections: Arc<RwLock<HashMap<String, StoredConnection>>>,
}
impl WebSocketManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn broadcast(&self, event: Event) -> usize {
        let message = match serde_json::to_string(&event) {
            Ok(msg) => axum::extract::ws::Utf8Bytes::from(msg),
            Err(e) => {
                error!("Couldn't broadcast event '{event:?}' due to serialization error: {e} ");
                return 0;
            }
        };

        let event_bit = EventKind::from(&event).to_bit();
        let connections = self.connections.read().await;
        let mut successful_sends = 0;
        let mut failed_connections = Vec::new();

        // Send events to all with matching events.
        for (id, (sender, event_mask)) in connections.iter() {
            if event_mask & event_bit != 0 {
                if sender.send(message.clone()).is_ok() {
                    successful_sends += 1;
                } else {
                    failed_connections.push(id.clone());
                }
            }
        }
        drop(connections);

        // Cleanup failed connections (read lock dropped before acquiring write).
        if !failed_connections.is_empty() {
            let mut connections = self.connections.write().await;
            for id in failed_connections {
                connections.remove(&id);
            }
        }
        successful_sends
    }

    pub async fn add_connection(
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
            let mut connections = self.connections.write().await;

            if !connections.contains_key(&id) {
                connections.insert(id.clone(), (tx, event_mask));
                return id;
            }
            drop(connections);
        }
    }

    pub async fn remove_connection(&self, id: &str) {
        self.connections.write().await.remove(id);
    }
}

// Called after the connection is upgraded.
pub async fn handle_websocket(connection: WebSocketConnection, manager: WebSocketManager) {
    let (mut sender, mut receiver) = connection.0.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<axum::extract::ws::Utf8Bytes>();

    // Add connection.
    let connection_id = manager.add_connection(tx, connection.1).await;
    debug!("WebSocket connection established: {connection_id}");

    // Writer task.
    let connection_id_for_tx = connection_id.clone();
    let (ping_tx, mut ping_rx) = mpsc::unbounded_channel();
    let tx_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Outgoing messages.
                msg = rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if sender.send(axum::extract::ws::Message::Text(msg)).await.is_err() {
                                break;
                            }
                        }
                        None => break // Channel closed
                    }
                },
                // Handle ping responses (pong messages).
                ping_data = ping_rx.recv() => {
                    match ping_data {
                        Some(data) => {
                            if sender.send(axum::extract::ws::Message::Pong(data)).await.is_err() {
                                break;
                            }
                        }
                        None => break // Channel closed
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
    manager.remove_connection(&connection_id_for_tx).await;
    debug!("WebSocket connection cleaned up: {connection_id_for_tx}");
}
