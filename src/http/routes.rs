use crate::http::types::{
    HttpResult, HttpSuccess, SuccessfulResponse, WebSocketQuery,
};
use crate::http::websocket::{handle_websocket, WebSocketConnection};
use crate::http::HttpState;
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::Response;

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/sys/version",
    tag = "System",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Version retrieved successfully", body = SuccessfulResponse<String>,
            example = json!({"success": true, "data": "1.0.0"}))
    )
))]
pub async fn sys_version(State(_state): State<HttpState>) -> HttpResult<String> {
    Ok(HttpSuccess(crate::VERSION.to_string()))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/sys/phone-number",
    tag = "System",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "System phone number retrieved successfully", body = SuccessfulResponse<Option<String>>,
            example = json!({"success": true, "data": "+1234567890"}))
    )
))]
pub async fn sys_phone_number(State(state): State<HttpState>) -> HttpResult<Option<String>> {
    Ok(HttpSuccess(state.config.phone_number.clone()))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/ws",
    tag = "WebSocket",
    params(WebSocketQuery),
    responses(
        (status = 101, description = "WebSocket connection established"),
        (status = 404, description = "WebSocket functionality is disabled")
    )
))]
pub async fn websocket_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<HttpState>,
    Query(query_params): Query<WebSocketQuery>,
) -> Result<Response, StatusCode> {
    let events = query_params.get_event_types();
    let response = match state.websocket {
        Some(manager) => ws.on_upgrade(|socket| {
            let connection: WebSocketConnection = (socket, events);
            handle_websocket(connection, manager)
        }),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body("Websocket functionality is disabled!".into())
            .unwrap_or_else(|_| Response::new("Internal Server Error".into())),
    };
    Ok(response)
}
