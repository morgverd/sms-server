use crate::http::types::{HttpError, HttpResult, HttpSuccess};
use crate::http::websocket::{handle_websocket, WebSocketConnection};
use crate::http::HttpState;
use crate::modem::types::{ModemRequest, ModemResponse};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use sms_pdu::pdu::{PduAddress, TypeOfNumber};
use std::str::FromStr;
use tracing_subscriber::EnvFilter;

macro_rules! modem_extract {
    (@inner $response:expr, $variant:ident) => {
        match $response {
            ModemResponse::$variant(data) => Ok(data),
            ModemResponse::Error(message) => Err(HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message
            }),
            other => Err(HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Unexpected response: expected {}, got {:?}", stringify!($variant), other),
            }),
        }
    };

    // Internal matching logic - extracts data from a struct variant
    (@inner $response:expr, $variant:ident { $($field:ident),+ }) => {
        match $response {
            ModemResponse::$variant { $($field),+ } => Ok(($($field,)+)),
            ModemResponse::Error(message) => Err(HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message
            }),
            other => Err(HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Unexpected response: expected {}, got {:?}", stringify!($variant), other),
            }),
        }
    };

    // Extract from an existing response
    ($response:expr => $($rest:tt)+) => {
        modem_extract!(@inner $response, $($rest)+)
    };

    // Send command and extract
    ($sms_manager:expr, $request:expr => $($rest:tt)+) => {{
        let response = $sms_manager
            .send_command($request)
            .await
            .map_err(|e| HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: e.to_string(),
            })?;

        modem_extract!(@inner response, $($rest)+)
    }};
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/db/messages",
    tag = "Database",
    summary = "Fetch SMS messages",
    description = "Retrieves SMS messages for a specific phone number from the database. Supports optional pagination.",
    security(("bearer_auth" = [])),
    request_body(
        content = crate::http::types::PhoneNumberFetchRequest,
        example = json!({"phone_number": "+1234567890", "limit": 50, "offset": 0, "reverse": false})
    ),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<Vec<sms_types::sms::SmsMessage>>))
    )
))]
pub async fn db_messages(
    State(state): State<HttpState>,
    Json(payload): Json<crate::http::types::PhoneNumberFetchRequest>,
) -> HttpResult<Vec<sms_types::sms::SmsMessage>> {
    let messages = state
        .sms_manager
        .borrow_database()
        .get_messages(
            &payload.phone_number,
            payload.limit,
            payload.offset,
            payload.reverse,
        )
        .await
        .map_err(|e| HttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        })?;

    Ok(HttpSuccess(messages))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/db/latest-numbers",
    tag = "Database",
    security(("bearer_auth" = [])),
    summary = "Get latest phone numbers",
    description = "Retrieves a list of phone numbers that have recently sent or received messages, along with their friendly names if set. Useful for populating a conversation list. Supports optional pagination.",
    request_body(
        content = Option<crate::http::types::GlobalFetchRequest>,
        example = json!({"limit": 50})
    ),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<Vec<sms_types::http::LatestNumberFriendlyNamePair>>))
    )
))]
pub async fn db_latest_numbers(
    State(state): State<HttpState>,
    Json(payload): Json<Option<crate::http::types::GlobalFetchRequest>>,
) -> HttpResult<Vec<sms_types::http::LatestNumberFriendlyNamePair>> {
    let (limit, offset, reverse) = match payload {
        Some(req) => (req.limit, req.offset, req.reverse),
        None => (None, None, false),
    };

    let latest_numbers = state
        .sms_manager
        .borrow_database()
        .get_latest_numbers(limit, offset, reverse)
        .await
        .map_err(|e| HttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        })?
        .into_iter()
        .map(sms_types::http::LatestNumberFriendlyNamePair::from)
        .collect();

    Ok(HttpSuccess(latest_numbers))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/db/delivery-reports",
    tag = "Database",
    summary = "Get delivery reports",
    description = "Retrieves delivery status reports for a specific sent message by its message ID. Returns information about whether the message was delivered, pending, or failed. There may be multiple delivery reports for delivery retries.",
    security(("bearer_auth" = [])),
    request_body(
        content = crate::http::types::MessageIdFetchRequest,
        example = json!({"message_id": 10, "limit": 1, "reverse": true})
    ),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<Vec<sms_types::sms::SmsDeliveryReport>>))
    )
))]
pub async fn db_delivery_reports(
    State(state): State<HttpState>,
    Json(payload): Json<crate::http::types::MessageIdFetchRequest>,
) -> HttpResult<Vec<sms_types::sms::SmsDeliveryReport>> {
    let delivery_reports = state
        .sms_manager
        .borrow_database()
        .get_delivery_reports(
            payload.message_id,
            payload.limit,
            payload.offset,
            payload.reverse,
        )
        .await
        .map_err(|e| HttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        })?;

    Ok(HttpSuccess(delivery_reports))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/db/friendly-names/set",
    tag = "Database",
    summary = "Set friendly name",
    description = "Associates a friendly name (contact name) with a phone number. This name will be returned alongside the phone number in other API responses.",
    security(("bearer_auth" = [])),
    request_body(
        content = crate::http::types::SetFriendlyNameRequest,
        example = json!({"phone_number": "+1234567890", "friendly_name": "Cool guy!"})
    ),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<bool>),
            example = json!({"success": true, "response": true}))
    )
))]
pub async fn db_friendly_names_set(
    State(state): State<HttpState>,
    Json(payload): Json<crate::http::types::SetFriendlyNameRequest>,
) -> HttpResult<bool> {
    let success = state
        .sms_manager
        .borrow_database()
        .update_friendly_name(payload.phone_number, payload.friendly_name)
        .await
        .map(|_| true)
        .map_err(|e| HttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        })?;

    Ok(HttpSuccess(success))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/db/friendly-names/get",
    tag = "Database",
    summary = "Get friendly name",
    description = "Retrieves the friendly name (contact name) associated with a phone number, if one has been set.",
    security(("bearer_auth" = [])),
    request_body(
        content = crate::http::types::GetFriendlyNameRequest,
        example = json!({"phone_number": "+1234567890"})
    ),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<Option<String>>),
            example = json!({"success": true, "response": "Cool guy!"}))
    )
))]
pub async fn db_friendly_names_get(
    State(state): State<HttpState>,
    Json(payload): Json<crate::http::types::GetFriendlyNameRequest>,
) -> HttpResult<Option<String>> {
    let friendly_name = state
        .sms_manager
        .borrow_database()
        .get_friendly_name(payload.phone_number)
        .await
        .map_err(|e| HttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        })?;

    Ok(HttpSuccess(friendly_name))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/sms/send",
    tag = "SMS",
    summary = "Send SMS message",
    description = "Sends an SMS message to the specified phone number. Supports flash messages (displayed immediately on the recipient's screen), custom validity periods, and configurable timeout. Returns the message ID and network reference ID on success.",
    security(("bearer_auth" = [])),
    request_body(
        content = crate::http::types::SendSmsRequest,
        example = json!({"to": "+1234567890", "content": "Hello! This is a test message.", "flash": true, "timeout": 10})
    ),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<sms_types::http::HttpSmsSendResponse>))
    )
))]
pub async fn sms_send(
    State(state): State<HttpState>,
    Json(payload): Json<crate::http::types::SendSmsRequest>,
) -> HttpResult<sms_types::http::HttpSmsSendResponse> {
    let address = PduAddress::from_str(&payload.to).map_err(|e| HttpError {
        status: StatusCode::BAD_REQUEST,
        message: e.to_string(),
    })?;

    if state.config.send_international_format_only
        && !matches!(
            address.type_addr.type_of_number,
            TypeOfNumber::International
        )
    {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: "Sending phone number must be in international format!".to_string(),
        });
    }

    // Quick-fix to make sure the number is valid before attempting.
    let to = address.to_string();
    match to.as_str() {
        "+" | "" => {
            return Err(HttpError {
                status: StatusCode::BAD_REQUEST,
                message: "Invalid phone number!".to_string(),
            })
        }
        _ => {}
    }

    // Create and send outgoing SMS message, handling unexpected return type.
    let outgoing = sms_types::sms::SmsOutgoingMessage {
        to,
        content: payload.content,
        flash: payload.flash,
        validity_period: payload.validity_period,
        timeout: payload.timeout,
    };
    let (message_id_opt, response) =
        state
            .sms_manager
            .send_sms(outgoing)
            .await
            .map_err(|e| HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: e.to_string(),
            })?;

    Ok(HttpSuccess(sms_types::http::HttpSmsSendResponse {
        message_id: message_id_opt.ok_or_else(|| HttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "Message sent but no message ID returned".to_string(),
        })?,
        reference_id: modem_extract!(response => SendResult)?,
    }))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/sms/network-status",
    tag = "SMS",
    summary = "Get network registration status",
    description = "Returns the current network registration status and technology type (e.g., 2G, 3G, 4G) for the modem.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<sms_types::http::HttpModemNetworkStatusResponse>),
            example = json!({"success": true, "response": {"registration": 0, "technology": 1}}))
    )
))]
pub async fn sms_get_network_status(
    State(state): State<HttpState>,
) -> HttpResult<sms_types::http::HttpModemNetworkStatusResponse> {
    let (registration, technology) = modem_extract!(
        state.sms_manager,
        ModemRequest::GetNetworkStatus => NetworkStatus { registration, technology }
    )?;
    Ok(HttpSuccess(
        sms_types::http::HttpModemNetworkStatusResponse {
            registration,
            technology,
        },
    ))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/sms/signal-strength",
    tag = "SMS",
    summary = "Get signal strength",
    description = "Returns the current signal strength (RSSI) and bit error rate (BER) from the modem. RSSI values typically range from 0-31, with higher values indicating stronger signal.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<sms_types::http::HttpModemSignalStrengthResponse>),
            example = json!({"success": true, "response": {"rssi": 17, "ber": 0}}))
    )
))]
pub async fn sms_get_signal_strength(
    State(state): State<HttpState>,
) -> HttpResult<sms_types::http::HttpModemSignalStrengthResponse> {
    let (rssi, ber) = modem_extract!(
        state.sms_manager,
        ModemRequest::GetSignalStrength => SignalStrength { rssi, ber }
    )?;
    Ok(HttpSuccess(
        sms_types::http::HttpModemSignalStrengthResponse { rssi, ber },
    ))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/sms/network-operator",
    tag = "SMS",
    summary = "Get network operator",
    description = "Returns information about the currently connected network operator, including the operator name and connection status.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<sms_types::http::HttpModemNetworkOperatorResponse>),
            example = json!({"success": true, "response": {"status": 0, "format": 0, "operator": "vodafone"}}))
    )
))]
pub async fn sms_get_network_operator(
    State(state): State<HttpState>,
) -> HttpResult<sms_types::http::HttpModemNetworkOperatorResponse> {
    let (status, format, operator) = modem_extract!(
        state.sms_manager,
        ModemRequest::GetNetworkOperator => NetworkOperator { status, format, operator }
    )?;
    Ok(HttpSuccess(
        sms_types::http::HttpModemNetworkOperatorResponse {
            status,
            format,
            operator,
        },
    ))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/sms/service-provider",
    tag = "SMS",
    summary = "Get service provider",
    description = "Returns the name of the SIM card's service provider (e.g., the mobile carrier name stored on the SIM).",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<String>),
            example = json!({"success": true, "response": "ASDA Mobile"}))
    )
))]
pub async fn sms_get_service_provider(State(state): State<HttpState>) -> HttpResult<String> {
    let service_provider = modem_extract!(
        state.sms_manager,
        ModemRequest::GetServiceProvider => ServiceProvider
    )?;
    Ok(HttpSuccess(service_provider))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/sms/battery-level",
    tag = "SMS",
    summary = "Get battery level",
    description = "Returns the current battery status, charge percentage, and voltage of the modem device. Only applicable for battery-powered modems, usually for GNSS warm starts.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<sms_types::http::HttpModemBatteryLevelResponse>),
            example = json!({"success": true, "response": {"status": 0, "charge": 71, "voltage": 3.972}}))
    )
))]
pub async fn sms_get_battery_level(
    State(state): State<HttpState>,
) -> HttpResult<sms_types::http::HttpModemBatteryLevelResponse> {
    let (status, charge, voltage) = modem_extract!(
        state.sms_manager,
        ModemRequest::GetBatteryLevel => BatteryLevel { status, charge, voltage }
    )?;
    Ok(HttpSuccess(
        sms_types::http::HttpModemBatteryLevelResponse {
            status,
            charge,
            voltage,
        },
    ))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/sms/device-info",
    tag = "SMS",
    summary = "Get device information",
    description = "Returns all modem information, this is more efficient than requesting each individually.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<sms_types::http::HttpSmsDeviceInfoResponse>))
    )
))]
pub async fn sms_get_device_info(
    State(state): State<HttpState>,
) -> HttpResult<sms_types::http::HttpSmsDeviceInfoResponse> {
    Ok(HttpSuccess(sms_types::http::HttpSmsDeviceInfoResponse {
        version: crate::VERSION.to_string(),
        phone_number: state.config.phone_number.clone(),
        service_provider: modem_extract!(state.sms_manager, ModemRequest::GetServiceProvider => ServiceProvider).ok(),
        network_operator: modem_extract!(state.sms_manager, ModemRequest::GetNetworkOperator => NetworkOperator { status, format, operator })
            .ok()
            .map(|(status, format, operator)| sms_types::http::HttpModemNetworkOperatorResponse { status, format, operator }),
        network_status: modem_extract!(state.sms_manager, ModemRequest::GetNetworkStatus => NetworkStatus { registration, technology })
            .ok()
            .map(|(registration, technology)| sms_types::http::HttpModemNetworkStatusResponse { registration, technology }),
        battery: modem_extract!(state.sms_manager, ModemRequest::GetBatteryLevel => BatteryLevel { status, charge, voltage })
            .ok()
            .map(|(status, charge, voltage)| sms_types::http::HttpModemBatteryLevelResponse { status, charge, voltage }),
        signal: modem_extract!(state.sms_manager, ModemRequest::GetSignalStrength => SignalStrength { rssi, ber })
            .ok()
            .map(|(rssi, ber)| sms_types::http::HttpModemSignalStrengthResponse { rssi, ber }),
    }))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/gnss/status",
    tag = "GNSS",
    summary = "Get GNSS fix status",
    description = "Returns the current GNSS fix status, indicating whether a position fix has been acquired and the type of fix (e.g., no fix, 2D fix, 3D fix).",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<sms_types::gnss::FixStatus>))
    )
))]
pub async fn gnss_get_status(
    State(state): State<HttpState>,
) -> HttpResult<sms_types::gnss::FixStatus> {
    let fix_status = modem_extract!(
        state.sms_manager,
        ModemRequest::GetGNSSStatus => GNSSStatus
    )?;
    Ok(HttpSuccess(fix_status))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/gnss/location",
    tag = "GNSS",
    summary = "Get GNSS location",
    description = "Returns the current GNSS position report. Requires a valid GNSS fix.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<sms_types::gnss::PositionReport>))
    )
))]
pub async fn gnss_get_location(
    State(state): State<HttpState>,
) -> HttpResult<sms_types::gnss::PositionReport> {
    let position_report = modem_extract!(
        state.sms_manager,
        ModemRequest::GetGNSSLocation => GNSSLocation
    )?;
    Ok(HttpSuccess(position_report))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/sys/version",
    tag = "System",
    summary = "Get server version",
    description = "Returns the current version string of the SMS server.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Version retrieved successfully", body = inline(crate::http::types::SuccessfulResponse<String>),
            example = json!({"success": true, "response": "1.0.0"}))
    )
))]
pub async fn sys_version(State(_state): State<HttpState>) -> HttpResult<String> {
    Ok(HttpSuccess(crate::VERSION.to_string()))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/sys/phone-number",
    tag = "System",
    summary = "Get configured phone number",
    description = "Returns the phone number configured for this SMS server, if one has been set in the configuration.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "System phone number retrieved successfully", body = inline(crate::http::types::SuccessfulResponse<Option<String>>),
            example = json!({"success": true, "response": "+1234567890"}))
    )
))]
pub async fn sys_phone_number(State(state): State<HttpState>) -> HttpResult<Option<String>> {
    Ok(HttpSuccess(state.config.phone_number.clone()))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/sys/set-log-level",
    tag = "System",
    summary = "Set log level",
    description = "Change the server's logging level at runtime. This can be useful for live debugging via journalctl etc.",
    security(("bearer_auth" = [])),
    request_body(
        content = crate::http::types::SetLogLevelRequest,
        example = json!({"level": "debug"})
    ),
    responses(
        (status = 200, body = inline(crate::http::types::SuccessfulResponse<bool>),
            example = json!({"success": true, "response": true}))
    )
))]
pub async fn sys_set_log_level(
    State(state): State<HttpState>,
    Json(payload): Json<crate::http::types::SetLogLevelRequest>,
) -> HttpResult<bool> {
    let filter = EnvFilter::from_str(&payload.level).map_err(|e| HttpError {
        status: StatusCode::BAD_REQUEST,
        message: e.to_string(),
    })?;

    tracing::log::info!("Setting log level to {filter} via API");
    let success = state
        .tracing_reload
        .reload(filter)
        .map(|_| true)
        .map_err(|e| HttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        })?;

    Ok(HttpSuccess(success))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/ws",
    tag = "WebSocket",
    summary = "WebSocket connection",
    description = "Establishes a WebSocket connection for receiving real-time events.",
    params(crate::http::types::WebSocketQuery),
    responses(
        (status = 101, description = "WebSocket connection established"),
        (status = 404, description = "WebSocket functionality is disabled")
    )
))]
pub async fn websocket_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<HttpState>,
    Query(query_params): Query<crate::http::types::WebSocketQuery>,
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
