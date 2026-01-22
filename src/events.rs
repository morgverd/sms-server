use crate::config::AppConfig;
use crate::webhooks::WebhookSender;
use sms_types::events::Event;
use tokio::task::JoinHandle;
use tracing::log::debug;

#[cfg(feature = "http-server")]
use crate::http::websocket::WebSocketManager;

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
    pub fn broadcast(&self, event: Event) {
        debug!("Broadcasting event: {event:?}");
        if let Some(webhooks) = &self.webhooks {
            webhooks.send(event.clone());
        }

        #[cfg(feature = "http-server")]
        if let Some(websocket) = &self.websocket {
            websocket.broadcast(event);
        }
    }
}
