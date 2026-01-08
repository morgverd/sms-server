use crate::config::ConfiguredWebhook;
use anyhow::{Context, Result};
use futures::{stream, StreamExt};
use reqwest::header::HeaderMap;
use reqwest::Client;
use sms_types::events::{Event, EventKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::log::{debug, error, info, warn};

const CONCURRENCY_LIMIT: usize = 10;
const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

fn client_builder(webhooks: &[ConfiguredWebhook]) -> Result<reqwest::ClientBuilder> {
    let builder = Client::builder();
    let certificate_paths: Vec<&PathBuf> = webhooks
        .iter()
        .filter_map(|w| w.certificate_path.as_ref())
        .collect();

    // If there are no certificates, return base builder.
    if certificate_paths.is_empty() {
        return Ok(builder);
    }

    #[cfg(not(any(feature = "tls-rustls", feature = "tls-native")))]
    {
        return Err(anyhow::anyhow!(
            "Webhook TLS configuration provided but no TLS features enabled. Compile with a TLS backend feature!"
        ));
    }

    #[cfg(any(feature = "tls-rustls", feature = "tls-native"))]
    {
        let mut builder = builder;

        // Configure TLS backend
        #[cfg(feature = "tls-rustls")]
        {
            builder = builder.use_rustls_tls();
        }

        #[cfg(feature = "tls-native")]
        {
            builder = builder.use_native_tls();
        }

        // Load and add certificate
        for certificate_path in certificate_paths {
            let certificate = load_certificate(certificate_path)?;
            builder = builder.add_root_certificate(certificate);
        }

        Ok(builder)
    }
}

#[cfg(any(feature = "tls-rustls", feature = "tls-native"))]
fn load_certificate(certificate_path: &std::path::Path) -> Result<reqwest::tls::Certificate> {
    let cert_data = std::fs::read(certificate_path)?;

    // Try to parse based on file extension first
    if let Some(ext) = certificate_path.extension().and_then(|s| s.to_str()) {
        match ext {
            "pem" => return Ok(reqwest::tls::Certificate::from_pem(&cert_data)?),
            "der" => return Ok(reqwest::tls::Certificate::from_der(&cert_data)?),
            "crt" => {
                if cert_data.starts_with(b"-----BEGIN") {
                    return Ok(reqwest::tls::Certificate::from_pem(&cert_data)?);
                } else {
                    return Ok(reqwest::tls::Certificate::from_der(&cert_data)?);
                }
            }
            _ => {}
        }
    }

    // Auto-detect format: try PEM first, then DER
    reqwest::tls::Certificate::from_pem(&cert_data)
        .or_else(|_| reqwest::tls::Certificate::from_der(&cert_data))
        .map_err(Into::into)
}

#[derive(Clone)]
pub struct WebhookSender {
    event_sender: mpsc::UnboundedSender<Event>,
}
impl WebhookSender {
    pub fn new(webhooks: Vec<ConfiguredWebhook>) -> (Self, JoinHandle<()>) {
        // Use an unbounded channel to ensure no webhooks are ever dropped.
        // The modem command channel is bound, so we should be fine from API spam.
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        let handle = tokio::spawn(async move {
            let worker = WebhookWorker::new(webhooks, event_receiver);
            worker.run().await;
        });

        let manager = Self { event_sender };
        (manager, handle)
    }

    pub fn send(&self, event: Event) {
        if let Err(e) = self.event_sender.send(event) {
            error!("Failed to queue webhook job: {e}");
        }
    }
}

type StoredWebhook = (ConfiguredWebhook, Option<HeaderMap>);

struct WebhookWorker {
    webhooks: Arc<[StoredWebhook]>,
    events_map: HashMap<EventKind, Vec<usize>>,
    event_receiver: mpsc::UnboundedReceiver<Event>,
    client: Client,
}
impl WebhookWorker {
    fn new(
        webhooks: Vec<ConfiguredWebhook>,
        event_receiver: mpsc::UnboundedReceiver<Event>,
    ) -> Self {
        let mut events_map: HashMap<EventKind, Vec<usize>> = HashMap::new();
        for (idx, webhook) in webhooks.iter().enumerate() {
            for event in &webhook.events {
                events_map.entry(*event).or_default().push(idx);
            }
        }

        let client = client_builder(&webhooks)
            .expect("Failed to create Webhooks Reqwest client builder!")
            .timeout(WEBHOOK_TIMEOUT)
            .build()
            .expect("Failed to build Webhooks Reqwest client!");

        Self {
            // Cache all webhook HeaderMaps now instead of re-creating each time.
            webhooks: webhooks
                .into_iter()
                .enumerate()
                .map(|(idx, webhook)| {
                    let headers = webhook.get_header_map().unwrap_or_else(|e| {
                        error!("Failed to create Webhook #{idx} HeaderMap with error: {e}");
                        None
                    });

                    (webhook, headers)
                })
                .collect::<Vec<StoredWebhook>>()
                .into(),

            events_map,
            event_receiver,
            client,
        }
    }

    async fn run(mut self) {
        info!("Starting webhook worker");
        while let Some(event) = self.event_receiver.recv().await {
            self.process(event).await;
        }
    }

    async fn process(&self, event: Event) {
        let webhook_indices = match self.events_map.get(&EventKind::from(&event)) {
            Some(indices) => indices.clone(),
            None => return,
        };

        let event = Arc::new(event);
        let webhooks = Arc::clone(&self.webhooks);

        stream::iter(webhook_indices.into_iter().enumerate())
            .map(|(task_idx, webhook_idx)| {
                let webhook = &webhooks[webhook_idx];
                let event = Arc::clone(&event);
                let client = &self.client;

                // TODO: Maybe re-queue failed webhooks?
                async move {
                    match Self::execute_webhook(webhook, client, &event).await {
                        Ok(()) => debug!(
                            "Webhook #{webhook_idx} for task #{task_idx} was sent successfully!"
                        ),
                        Err(e) => warn!(
                            "Failed to send Webhook #{webhook_idx} for task #{task_idx} with error: {e}"
                        ),
                    }
                }
            })
            .buffer_unordered(CONCURRENCY_LIMIT)
            .for_each(|_| async {})
            .await;
    }

    async fn execute_webhook(
        (webhook, headers): &StoredWebhook,
        client: &Client,
        event: &Event,
    ) -> Result<()> {
        let mut request = client.post(&webhook.url).json(event);

        if let Some(headers) = headers {
            request = request.headers(headers.clone());
        }

        let status = request
            .send()
            .await
            .with_context(|| "Network error")?
            .status();

        match webhook.expected_status {
            Some(expected) if status.as_u16() != expected => {
                anyhow::bail!("Got {} expected {}!", status.as_u16(), expected);
            }
            None if !status.is_success() => {
                anyhow::bail!("Unsuccessful status {}", status);
            }
            _ => Ok(()),
        }
    }
}
