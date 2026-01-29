mod routes;
mod types;
pub mod websocket;

#[cfg(feature = "openapi")]
mod openapi;

use crate::config::HTTPConfig;
use crate::http::routes::*;
use crate::http::types::HttpError;
use crate::http::websocket::WebSocketManager;
use crate::sms::SMSManager;
use crate::TracingReloadHandle;
use anyhow::{bail, Result};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::routing::{get, post};
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::log::{debug, warn};

#[cfg(feature = "openapi")]
use utoipa::OpenApi;

#[cfg(feature = "sentry")]
use sentry::integrations::tower::{NewSentryLayer, SentryHttpLayer};

#[derive(Clone)]
pub struct HttpState {
    pub sms_manager: SMSManager,
    pub config: HTTPConfig,
    pub tracing_reload: TracingReloadHandle,
    pub websocket: Option<WebSocketManager>,
}

async fn auth_middleware(
    axum::extract::State(expected_token): axum::extract::State<String>,
    headers: axum::http::HeaderMap,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, HttpError> {
    let auth_header = headers.get("authorization").ok_or(HttpError {
        status: StatusCode::UNAUTHORIZED,
        message: "Missing authorization header".to_string(),
    })?;

    let auth_str = auth_header.to_str().map_err(|_| HttpError {
        status: StatusCode::BAD_REQUEST,
        message: "Invalid authorization header".to_string(),
    })?;

    let token = auth_str.strip_prefix("Bearer ").unwrap_or(auth_str).trim();
    if token != expected_token {
        return Err(HttpError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid token".to_string(),
        });
    }

    Ok(next.run(request).await)
}

pub fn create_app(
    config: HTTPConfig,
    websocket: Option<WebSocketManager>,
    sms_manager: SMSManager,
    _sentry: bool,
    _tracing_reload: TracingReloadHandle,
) -> Result<axum::Router> {
    let mut router = axum::Router::new()
        .route("/db/messages", post(db_messages))
        .route("/db/latest-numbers", post(db_latest_numbers))
        .route("/db/delivery-reports", post(db_delivery_reports))
        .route("/db/friendly-names/set", post(db_friendly_names_set))
        .route("/db/friendly-names/get", post(db_friendly_names_get))
        .route("/sms/send", post(sms_send))
        .route("/sms/network-status", get(sms_get_network_status))
        .route("/sms/signal-strength", get(sms_get_signal_strength))
        .route("/sms/network-operator", get(sms_get_network_operator))
        .route("/sms/service-provider", get(sms_get_service_provider))
        .route("/sms/battery-level", get(sms_get_battery_level))
        .route("/sms/device-info", get(sms_get_device_info))
        .route("/gnss/status", get(gnss_get_status))
        .route("/gnss/location", get(gnss_get_location))
        .route("/sys/phone-number", get(sys_phone_number))
        .route("/sys/version", get(sys_version))
        .route("/sys/set-log-level", post(sys_set_log_level))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-version"),
            HeaderValue::from_static(crate::VERSION),
        ))
        .layer(ServiceBuilder::new().layer(CorsLayer::permissive()));

    // Add optional websocket route if there is a manager.
    if websocket.is_some() {
        debug!("Adding WebSocket broadcaster HTTP route!");
        router = router.route("/ws", get(websocket_upgrade));
    }

    // Add optional authentication middleware.
    if config.require_authentication {
        match std::env::var("SMS_HTTP_AUTH_TOKEN") {
            Ok(token) => {
                debug!("Adding HTTP authentication middleware!");
                router = router.layer(
                    axum::middleware::from_fn_with_state(token, auth_middleware)
                );
            },
            Err(_) => bail!("Missing required SMS_HTTP_AUTH_TOKEN environment variable, and require_authentication is enabled!")
        }
    } else {
        warn!("Serving HTTP without authentication middleware, as require_authentication is disabled!");
    }

    #[cfg(feature = "openapi")]
    {
        debug!("Adding OpenAPI SwaggerUi at /docs!");
        router = router.merge(
            utoipa_swagger_ui::SwaggerUi::new("/docs")
                .url("/docs/openapi.json", openapi::ApiDoc::openapi()),
        );
    }

    // If Sentry is enabled, include axum integration layers.
    #[cfg(feature = "sentry")]
    if _sentry {
        debug!("Adding Sentry HTTP layer!");
        router = router
            .layer(
                ServiceBuilder::new()
                    .layer(NewSentryLayer::<axum::http::Request<axum::body::Body>>::new_from_top()),
            )
            .layer(ServiceBuilder::new().layer(SentryHttpLayer::new().enable_transaction()))
    }

    // Shared HTTP route state.
    let state = HttpState {
        sms_manager,
        config,
        tracing_reload: _tracing_reload,
        websocket,
    };
    Ok(router.with_state(state))
}
