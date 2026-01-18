use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct WebhookPayload {
    #[serde(rename = "type")]
    pub webhook_type: String,
    pub data: WebhookMessage,
}

#[derive(Debug, Deserialize)]
pub struct WebhookMessage {
    pub phone_number: String,
    pub message_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ChatGPTCompletionRequest {
    pub model: &'static str,
    pub temperature: f32,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize)]
pub struct ChatGPTCompletionResponse {
    pub choices: Vec<ChatGPTCompletionChoice>,
}

#[derive(Debug, Deserialize)]
pub struct ChatGPTCompletionChoice {
    pub message: ChatMessage,
}

#[derive(Serialize)]
pub struct SendReplyRequest {
    pub to: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug)]
pub struct MessageTask {
    pub phone_number: String,
    pub message_content: String,
}
