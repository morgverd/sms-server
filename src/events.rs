use crate::config::AppConfig;
use crate::webhooks::WebhookSender;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use sms_types::gnss::PositionReport;
use sms_types::modem::ModemStatusUpdateState;
use sms_types::sms::{SmsMessage, SmsPartialDeliveryReport};
use tokio::task::JoinHandle;
use tracing::log::debug;

#[cfg(feature = "http-server")]
use crate::http::websocket::WebSocketManager;

#[derive(Eq, PartialEq, Hash, Debug, Clone, Copy, Deserialize)]
pub enum EventType {
    #[serde(rename = "incoming")]
    IncomingMessage,

    #[serde(rename = "outgoing")]
    OutgoingMessage,

    #[serde(rename = "delivery")]
    DeliveryReport,

    #[serde(rename = "modem_status_update")]
    ModemStatusUpdate,

    #[serde(rename = "gnss_position_report")]
    GNSSPositionReport,
}
#[cfg_attr(not(feature = "http-server"), allow(dead_code))]
impl EventType {
    pub const COUNT: usize = 5;

    #[inline]
    pub const fn to_bit(self) -> u8 {
        match self {
            EventType::IncomingMessage => 1 << 0,    // 0b00001
            EventType::OutgoingMessage => 1 << 1,    // 0b00010
            EventType::DeliveryReport => 1 << 2,     // 0b00100
            EventType::ModemStatusUpdate => 1 << 3,  // 0b01000
            EventType::GNSSPositionReport => 1 << 4, // 0b10000
        }
    }

    #[inline]
    pub const fn all_bits() -> u8 {
        (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4) // 0b11111
    }

    #[inline]
    pub fn events_to_mask(events: &[EventType]) -> u8 {
        events.iter().fold(0, |acc, event| acc | event.to_bit())
    }
}
impl TryFrom<&str> for EventType {
    type Error = anyhow::Error;

    #[inline]
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "incoming" => Ok(EventType::IncomingMessage),
            "outgoing" => Ok(EventType::OutgoingMessage),
            "delivery" => Ok(EventType::DeliveryReport),
            "modem_status_update" => Ok(EventType::ModemStatusUpdate),
            "gnss_position_report" => Ok(EventType::GNSSPositionReport),
            _ => Err(anyhow!("Unknown event type {}", value)),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum Event {
    #[serde(rename = "incoming")]
    IncomingMessage(SmsMessage),

    #[serde(rename = "outgoing")]
    OutgoingMessage(SmsMessage),

    #[serde(rename = "delivery")]
    DeliveryReport {
        message_id: i64,
        report: SmsPartialDeliveryReport,
    },

    #[serde(rename = "modem_status_update")]
    ModemStatusUpdate {
        previous: ModemStatusUpdateState,
        current: ModemStatusUpdateState,
    },

    #[serde(rename = "gnss_position_report")]
    GNSSPositionReport(PositionReport),
}
impl Event {
    #[inline]
    pub fn to_event_type(&self) -> EventType {
        match self {
            Event::IncomingMessage(_) => EventType::IncomingMessage,
            Event::OutgoingMessage(_) => EventType::OutgoingMessage,
            Event::DeliveryReport { .. } => EventType::DeliveryReport,
            Event::ModemStatusUpdate { .. } => EventType::ModemStatusUpdate,
            Event::GNSSPositionReport(_) => EventType::GNSSPositionReport,
        }
    }
}

#[derive(Clone)]
pub struct EventBroadcaster {
    pub webhooks: Option<WebhookSender>,

    #[cfg(feature = "http-server")]
    pub websocket: Option<WebSocketManager>,
}
impl EventBroadcaster {
    pub fn new(config: &AppConfig) -> (Option<Self>, Option<JoinHandle<()>>) {
        let (webhook_sender, webhook_handle) = config
            .webhooks
            .clone()
            .map(WebhookSender::new)
            .map_or((None, None), |(sender, handle)| {
                (Some(sender), Some(handle))
            });

        #[cfg(feature = "http-server")]
        let websocket = config.http.websocket_enabled.then(WebSocketManager::new);

        #[cfg(feature = "http-server")]
        let is_enabled = webhook_sender.is_some() || websocket.is_some();

        #[cfg(not(feature = "http-server"))]
        let is_enabled = webhook_sender.is_some();

        (
            if is_enabled {
                Some(EventBroadcaster {
                    webhooks: webhook_sender,

                    #[cfg(feature = "http-server")]
                    websocket,
                })
            } else {
                None
            },
            webhook_handle,
        )
    }

    #[inline]
    pub async fn broadcast(&self, event: Event) {
        debug!("Broadcasting event: {event:?}");
        if let Some(webhooks) = &self.webhooks {
            webhooks.send(event.clone());
        }

        #[cfg(feature = "http-server")]
        if let Some(websocket) = &self.websocket {
            websocket.broadcast(event).await;
        }
    }
}
