mod types;

use crate::types::*;
use axum::Router;
use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::Json as ResponseJson;
use axum::routing::post;
use dashmap::DashMap;
use reqwest::Client;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, info, instrument, warn};

const CHATGPT_MODEL: &str = "gpt-4.1-mini";
const HISTORY_LIMIT: usize = 20;
const CHATGPT_TEMPERATURE: f32 = 0.8;
const CHATGPT_SYSTEM_PROMPT: &str = "You are an SMS assistant named Dexter, Always reply in short, clear SMS-style messages—never write more than 2-3 sentences per reply. Keep your tone friendly, upbeat, and a little bit witty, like a helpful buddy. Use contractions, emojis (if appropriate), and text as real people do via SMS. Never use formal or overly technical language. No long explanations or paragraphs—keep it brief but helpful! Do not reference that you are an AI or digital assistant. Always sound personable and natural.";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(thiserror::Error, Debug)]
enum AppError {
    #[error("Missing required {0} environment variable!")]
    MissingEnvironmentVariable(&'static str),
    #[error("OpenAI API error: {0}")]
    OpenAI(String),
    #[error("SMS API error: {0}")]
    Sms(String),
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

type Result<T> = std::result::Result<T, AppError>;

#[derive(Clone)]
struct AppState {
    message_history: Arc<Mutex<HashMap<String, VecDeque<ChatMessage>>>>,
    phone_queues: Arc<DashMap<String, mpsc::UnboundedSender<MessageTask>>>,
    sms_send_url: String,
    sms_send_auth: Option<String>,
    openai_key: String,
    http_client: Client,
}

impl AppState {
    fn from_env() -> Result<Self> {
        let http_client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("Failed to create HTTP client");

        let state = Self {
            message_history: Arc::new(Mutex::new(HashMap::new())),
            phone_queues: Arc::new(DashMap::new()),
            sms_send_url: env::var("SMS_SEND_URL")
                .map_err(|_| AppError::MissingEnvironmentVariable("SMS_SEND_URL"))?,
            sms_send_auth: env::var("SMS_SEND_AUTH").ok(),
            openai_key: env::var("OPENAI_KEY")
                .map_err(|_| AppError::MissingEnvironmentVariable("OPENAI_KEY"))?,
            http_client,
        };
        Ok(state)
    }

    async fn get_or_create_queue(&self, phone_number: &str) -> mpsc::UnboundedSender<MessageTask> {
        // Use existing queue if one exists
        if let Some(sender) = self.phone_queues.get(phone_number) {
            return sender.clone();
        }

        // Create new queue for this phone number
        let (tx, mut rx) = mpsc::unbounded_channel::<MessageTask>();
        let phone_number_clone = phone_number.to_string();
        let queues_ref = Arc::clone(&self.phone_queues);
        let state_clone = self.clone();

        // Insert the sender into the map
        self.phone_queues
            .insert(phone_number.to_string(), tx.clone());

        // Spawn worker task for this phone number
        tokio::spawn(async move {
            debug!(
                "Started queue worker for phone number: {}",
                phone_number_clone
            );

            while let Some(task) = rx.recv().await {
                debug!("Processing queued message for {}", task.phone_number);

                if let Err(e) = process_message(
                    state_clone.clone(),
                    task.phone_number.clone(),
                    task.message_content,
                )
                .await
                {
                    error!("Failed to process message for {}: {}", task.phone_number, e);
                }
            }

            // Clean up the queue when the worker shuts down
            debug!("Queue worker shutting down for: {}", phone_number_clone);
            queues_ref.remove(&phone_number_clone);
        });

        tx
    }

    /// Adds a message to history and returns a snapshot of the current conversation.
    #[instrument(skip(self, message), fields(phone_number = %phone_number))]
    async fn add_message_and_get_history(
        &self,
        phone_number: &str,
        message: ChatMessage,
    ) -> Vec<ChatMessage> {
        let mut history_guard = self.message_history.lock().await;
        let messages = history_guard
            .entry(phone_number.to_string())
            .or_insert_with(|| VecDeque::with_capacity(HISTORY_LIMIT));

        messages.push_back(message);
        Self::trim_history(messages);

        messages.iter().cloned().collect()
    }

    /// Adds a message to existing conversation history.
    #[instrument(skip(self, message), fields(phone_number = %phone_number))]
    async fn add_message(&self, phone_number: &str, message: ChatMessage) {
        let mut history_guard = self.message_history.lock().await;
        if let Some(messages) = history_guard.get_mut(phone_number) {
            messages.push_back(message);
            Self::trim_history(messages);
        }
    }

    /// Get a string message reply from ChatGPT with history snapshot.
    #[instrument(skip(self, messages))]
    async fn get_reply(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let system_message = ChatMessage {
            role: "system".to_string(),
            content: CHATGPT_SYSTEM_PROMPT.to_string(),
        };

        // Create new message set with system prompt.
        let mut all_messages = Vec::with_capacity(messages.len() + 1);
        all_messages.push(system_message);
        all_messages.extend(messages);

        // Create request payload.
        let request_body = ChatGPTCompletionRequest {
            model: CHATGPT_MODEL,
            temperature: CHATGPT_TEMPERATURE,
            messages: all_messages,
        };

        // Send chat completion request with history (including optional authorization).
        let mut builder = self
            .http_client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.openai_key))
            .header("Content-Type", "application/json")
            .json(&request_body);

        if let Some(auth) = &self.sms_send_auth {
            builder = builder.header("Authorization", auth);
        }

        // Send SMS message request with authorization header.
        debug!("Sending request to ChatGPT API");
        match builder.send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<ChatGPTCompletionResponse>().await {
                        Ok(chat_response) => {
                            if let Some(choice) = chat_response.choices.first() {
                                debug!("Successfully received ChatGPT response");
                                Ok(choice.message.content.clone())
                            } else {
                                Err(AppError::OpenAI("No choices in response".to_string()))
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse ChatGPT response: {}", e);
                            Err(AppError::OpenAI(format!("Parse error: {}", e)))
                        }
                    }
                } else {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    error!("ChatGPT API error: {} - {}", status, error_text);
                    Err(AppError::OpenAI(format!("{}: {}", status, error_text)))
                }
            }
            Err(e) => {
                error!("Failed to call ChatGPT API: {}", e);
                Err(AppError::Network(e))
            }
        }
    }

    /// Send the ChatGPT reply back via SMS API.
    #[instrument(skip(self), fields(phone_number = %phone_number, reply_length = reply.len()))]
    async fn send_reply(&self, phone_number: String, reply: String) -> Result<()> {
        let request_body = SendReplyRequest {
            to: phone_number.clone(),
            content: reply.clone(),
        };

        match self
            .http_client
            .post(&self.sms_send_url)
            .json(&request_body)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    debug!("Successfully sent reply to {}", phone_number);
                    Ok(())
                } else {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    error!("SMS API error: {} - {}", status, error_text);
                    Err(AppError::Sms(format!("{}: {}", status, error_text)))
                }
            }
            Err(e) => {
                error!("Failed to call SMS API: {}", e);
                Err(AppError::Network(e))
            }
        }
    }

    /// Clears all message history for a phone number.
    #[instrument(skip(self), fields(phone_number = %phone_number))]
    async fn clear_history(&self, phone_number: &str) -> usize {
        self.message_history
            .lock()
            .await
            .remove(phone_number)
            .map(|removed| removed.len())
            .unwrap_or(0)
    }

    /// Trims history to stay within limits.
    fn trim_history(messages: &mut VecDeque<ChatMessage>) {
        while messages.len() > HISTORY_LIMIT {
            messages.pop_front();
        }
    }
}

#[instrument(skip(state))]
async fn process_message(
    state: AppState,
    phone_number: String,
    message_content: String,
) -> Result<()> {
    // Check if this is a history clear command.
    if message_content.trim() == "#" {
        debug!("Received history clear command from {}", phone_number);

        let count = state.clear_history(&phone_number).await;
        let reply = format!("History cleared ({} messages)! Starting fresh.", count);

        if let Err(e) = state.send_reply(phone_number, reply).await {
            error!("Failed to send history clear confirmation: {}", e);
            return Err(e);
        }
        return Ok(());
    }

    // Store incoming message and get history.
    let incoming_message = ChatMessage {
        role: "user".to_string(),
        content: message_content,
    };
    let history_snapshot = state
        .add_message_and_get_history(&phone_number, incoming_message)
        .await;

    // Generate reply from ChatGPT.
    let reply = state.get_reply(history_snapshot).await.unwrap_or_else(|e| {
        error!("Failed to get ChatGPT reply: {}", e);
        match e {
            AppError::OpenAI(_) => "Sorry, the AI service is currently unavailable!".to_string(),
            AppError::Network(_) => "Sorry, I couldn't connect to the AI service!".to_string(),
            _ => "Sorry, there was an error processing your message!".to_string(),
        }
    });

    // Store outgoing message.
    let outgoing_message = ChatMessage {
        role: "assistant".to_string(),
        content: reply.clone(),
    };
    state.add_message(&phone_number, outgoing_message).await;

    // Finally, send the reply.
    if let Err(e) = state.send_reply(phone_number, reply).await {
        error!("Failed to send SMS reply: {}", e);
        return Err(e);
    }

    Ok(())
}

#[instrument(skip(state, payload))]
async fn http_webhook(
    State(state): State<AppState>,
    Json(payload): Json<WebhookPayload>,
) -> std::result::Result<StatusCode, (StatusCode, ResponseJson<ErrorResponse>)> {
    if payload.webhook_type != "incoming" {
        warn!(
            "Received non-incoming webhook type: {}",
            payload.webhook_type
        );
        return Err((
            StatusCode::BAD_REQUEST,
            ResponseJson(ErrorResponse {
                error: "Invalid webhook type".to_string(),
            }),
        ));
    }

    // Ignore non-international numbers such as carrier numbers.
    let phone_number = payload.data.phone_number;
    if !phone_number.starts_with("+") {
        warn!(
            "Discarding incoming non international number format: {}",
            phone_number
        );
        return Ok(StatusCode::OK);
    }

    let message_content = payload.data.message_content.trim().to_string();
    debug!(
        "Received message from {}, queuing for processing",
        phone_number
    );

    // Send task to queue for this number.
    let sender = state.get_or_create_queue(&phone_number).await;
    let task = MessageTask {
        phone_number: phone_number.clone(),
        message_content,
    };
    if let Err(_) = sender.send(task) {
        error!(
            "Failed to queue message for {}: receiver dropped",
            phone_number
        );
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            ResponseJson(ErrorResponse {
                error: "Failed to queue message".to_string(),
            }),
        ));
    }

    debug!("Message queued successfully for {}", phone_number);
    Ok(StatusCode::OK)
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let state = AppState::from_env()?;
    let app = Router::new()
        .route("/webhook", post(http_webhook))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:3001").await?;

    info!("Starting HTTP listener @ 127.0.0.1:3001");
    axum::serve(listener, app).await?;

    Ok(())
}
