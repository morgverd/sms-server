use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use sms_types::events::EventKind;
use std::collections::HashSet;

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SuccessfulResponse<T> {
    pub success: bool,
    pub response: T,
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorResponse {
    #[cfg_attr(feature = "openapi", schema(default = false))]
    pub success: bool,
    pub error: String,
}

pub struct HttpSuccess<T>(pub T);
impl<T: Serialize> IntoResponse for HttpSuccess<T> {
    fn into_response(self) -> Response {
        Json(SuccessfulResponse {
            success: true,
            response: self.0,
        })
        .into_response()
    }
}

pub struct HttpError {
    pub status: StatusCode,
    pub message: String,
}
impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                success: false,
                error: self.message,
            }),
        )
            .into_response()
    }
}

pub type HttpResult<T> = Result<HttpSuccess<T>, HttpError>;

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PhoneNumberFetchRequest {
    pub phone_number: String,

    #[serde(default)]
    pub limit: Option<u64>,

    #[serde(default)]
    pub offset: Option<u64>,

    #[serde(default)]
    pub reverse: bool,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MessageIdFetchRequest {
    pub message_id: i64,

    #[serde(default)]
    pub limit: Option<u64>,

    #[serde(default)]
    pub offset: Option<u64>,

    #[serde(default)]
    pub reverse: bool,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GlobalFetchRequest {
    #[serde(default)]
    pub limit: Option<u64>,

    #[serde(default)]
    pub offset: Option<u64>,

    #[serde(default)]
    pub reverse: bool,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SendSmsRequest {
    pub to: String,
    pub content: String,

    #[serde(default)]
    pub flash: Option<bool>,

    #[serde(default)]
    pub validity_period: Option<u8>,

    #[serde(default)]
    pub timeout: Option<u32>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetLogLevelRequest {
    pub level: String,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetFriendlyNameRequest {
    pub phone_number: String,
    pub friendly_name: Option<String>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GetFriendlyNameRequest {
    pub phone_number: String,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
pub struct WebSocketQuery {
    pub events: Option<String>,
}
impl WebSocketQuery {
    pub fn get_event_types(&self) -> Option<Vec<EventKind>> {
        let events_str = self.events.as_ref()?;
        if events_str == "*" {
            return None;
        }

        let events: Vec<EventKind> = events_str
            .split(',')
            .filter_map(|s| EventKind::try_from(s.trim()).ok())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        // If there are none or all, accept all events by applying no filter
        let size = events.len();
        if size == 0 || size == EventKind::COUNT {
            return None;
        }

        Some(events)
    }
}

#[cfg(test)]
mod websocket_query_tests {
    use super::*;

    #[test]
    fn test_returns_none() {
        let query = WebSocketQuery {
            events: Some("*".to_string()),
        };
        assert_eq!(query.get_event_types(), None);

        let query = WebSocketQuery { events: None };
        assert_eq!(query.get_event_types(), None);

        let query = WebSocketQuery {
            events: Some("".to_string()),
        };
        assert_eq!(query.get_event_types(), None);

        let query = WebSocketQuery {
            events: Some("invalid1,invalid2,invalid3".to_string()),
        };
        assert_eq!(query.get_event_types(), None);

        let query = WebSocketQuery {
            events: Some(" , , ".to_string()),
        };
        assert_eq!(query.get_event_types(), None);

        // All valid event types
        let query = WebSocketQuery {
            events: Some(
                "incoming,outgoing,delivery,modem_status_update,gnss_position_report".to_string(),
            ),
        };
        assert_eq!(query.get_event_types(), None);
    }

    #[test]
    fn test_parsing_and_filtering() {
        // Single valid
        let query = WebSocketQuery {
            events: Some("incoming".to_string()),
        };
        let result = query.get_event_types().unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains(&EventKind::IncomingMessage));

        // Duplicates
        let query = WebSocketQuery {
            events: Some("incoming,outgoing,incoming,delivery,outgoing".to_string()),
        };
        let result = query.get_event_types().unwrap();
        assert_eq!(result.len(), 3);
        assert!(result.contains(&EventKind::IncomingMessage));
        assert!(result.contains(&EventKind::OutgoingMessage));
        assert!(result.contains(&EventKind::DeliveryReport));

        // Mixed valid and invalid events with whitespace
        let query = WebSocketQuery {
            events: Some(" incoming , invalid_event , outgoing , unknown, delivery ".to_string()),
        };
        let result = query.get_event_types().unwrap();
        assert_eq!(result.len(), 3);
        assert!(result.contains(&EventKind::IncomingMessage));
        assert!(result.contains(&EventKind::OutgoingMessage));
        assert!(result.contains(&EventKind::DeliveryReport));

        let query = WebSocketQuery {
            events: Some(",incoming,,outgoing,".to_string()),
        };
        let result = query.get_event_types().unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&EventKind::IncomingMessage));
        assert!(result.contains(&EventKind::OutgoingMessage));
    }
}
