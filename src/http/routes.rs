use crate::http::types::{
    GetFriendlyNameRequest, GlobalFetchRequest, HttpResponse, MessageIdFetchRequest,
    PhoneNumberFetchRequest, SendSmsRequest, SendSmsResponse, SetFriendlyNameRequest,
    SetLogLevelRequest, SmsDeviceInfo, WebSocketQuery,
};
use crate::http::websocket::{handle_websocket, WebSocketConnection};
use crate::http::{get_modem_json_result, HttpState};
use crate::modem::types::{ModemRequest, ModemResponse};
use anyhow::{anyhow, bail};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::Response;
use sms_pdu::pdu::{PduAddress, TypeOfNumber};
use sms_types::sms::{SmsDeliveryReport, SmsMessage, SmsOutgoingMessage};
use std::str::FromStr;
use tracing_subscriber::EnvFilter;

macro_rules! http_response_handler {
    ($result:expr) => {
        match $result {
            Ok(data) => Ok(axum::Json(HttpResponse {
                success: true,
                response: Some(data),
                error: None,
            })),
            Err(e) => Err((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(HttpResponse {
                    success: false,
                    response: None,
                    error: Some(e.to_string()),
                }),
            )),
        }
    };
}

macro_rules! http_get_handler {
    (
        $fn_name:ident,
        $response_type:ty,
        |$state:ident| $callback:block
    ) => {
        pub async fn $fn_name(
            axum::extract::State($state): axum::extract::State<crate::http::HttpState>,
        ) -> crate::http::types::JsonResult<$response_type> {
            async fn inner($state: crate::http::HttpState) -> anyhow::Result<$response_type> {
                $callback
            }

            let result = inner($state).await;
            http_response_handler!(result)
        }
    };
}

macro_rules! http_post_handler {
    (
        $fn_name:ident,
        Option<$request_type:ty>,
        $response_type:ty,
        |$state:ident, $payload:ident| $callback:block
    ) => {
        pub async fn $fn_name(
            axum::extract::State($state): axum::extract::State<crate::http::HttpState>,
            payload: Option<axum::Json<$request_type>>,
        ) -> crate::http::types::JsonResult<$response_type> {
            async fn inner(
                $state: crate::http::HttpState,
                $payload: Option<$request_type>,
            ) -> anyhow::Result<$response_type> {
                $callback
            }

            let $payload = payload.map(|json| json.0);
            let result = inner($state, $payload).await;
            http_response_handler!(result)
        }
    };
    (
        $fn_name:ident,
        $request_type:ty,
        $response_type:ty,
        |$state:ident, $payload:ident| $callback:block
    ) => {
        pub async fn $fn_name(
            axum::extract::State($state): axum::extract::State<crate::http::HttpState>,
            axum::Json($payload): axum::Json<$request_type>,
        ) -> crate::http::types::JsonResult<$response_type> {
            async fn inner(
                $state: crate::http::HttpState,
                $payload: $request_type,
            ) -> anyhow::Result<$response_type> {
                $callback
            }

            let result = inner($state, $payload).await;
            http_response_handler!(result)
        }
    };
}

macro_rules! http_modem_handler {
    ($fn_name:ident, $modem_req:expr) => {
        pub async fn $fn_name(
            State(state): State<crate::http::HttpState>,
        ) -> crate::http::types::JsonResult<crate::modem::types::ModemResponse> {
            get_modem_json_result(state, $modem_req).await
        }
    };
}

macro_rules! modem_extract {
    ($sms_manager:expr, $request:expr => $variant:ident) => {
        match $sms_manager.send_command($request).await {
            Ok(ModemResponse::$variant(data)) => Some(data),
            _ => None
        }
    };
    ($sms_manager:expr, $request:expr => $variant:ident { $($field:ident),+ }) => {
        match $sms_manager.send_command($request).await {
            Ok(ModemResponse::$variant { $($field),+ }) => Some(($($field,)+)),
            _ => None
        }
    };
}

http_post_handler!(
    db_sms,
    PhoneNumberFetchRequest,
    Vec<SmsMessage>,
    |state, payload| {
        state
            .sms_manager
            .borrow_database()
            .get_messages(
                &payload.phone_number,
                payload.limit,
                payload.offset,
                payload.reverse,
            )
            .await
    }
);

http_post_handler!(
    db_delivery_reports,
    MessageIdFetchRequest,
    Vec<SmsDeliveryReport>,
    |state, payload| {
        state
            .sms_manager
            .borrow_database()
            .get_delivery_reports(
                payload.message_id,
                payload.limit,
                payload.offset,
                payload.reverse,
            )
            .await
    }
);

http_post_handler!(
    db_latest_numbers,
    Option<GlobalFetchRequest>,
    Vec<(String, Option<String>)>,
    |state, payload| {
        let (limit, offset, reverse) = match payload {
            Some(req) => (req.limit, req.offset, req.reverse),
            None => (None, None, false),
        };

        state
            .sms_manager
            .borrow_database()
            .get_latest_numbers(limit, offset, reverse)
            .await
    }
);

http_post_handler!(
    friendly_names_set,
    SetFriendlyNameRequest,
    bool,
    |state, payload| {
        state
            .sms_manager
            .borrow_database()
            .update_friendly_name(payload.phone_number, payload.friendly_name)
            .await
            .map(|_| true)
    }
);

http_post_handler!(
    friendly_names_get,
    GetFriendlyNameRequest,
    Option<String>,
    |state, payload| {
        state
            .sms_manager
            .borrow_database()
            .get_friendly_name(payload.phone_number)
            .await
    }
);

http_post_handler!(
    sms_send,
    SendSmsRequest,
    SendSmsResponse,
    |state, payload| {
        let address = PduAddress::from_str(&payload.to)?;
        if state.config.send_international_format_only
            && !matches!(
                address.type_addr.type_of_number,
                TypeOfNumber::International
            )
        {
            bail!("Sending phone number must be in international format!");
        }

        // Quick-fix to make sure the number is valid before attempting.
        let to = address.to_string();
        match to.as_str() {
            "+" | "" => bail!("Invalid phone number!"),
            _ => {}
        }

        let outgoing = SmsOutgoingMessage {
            to,
            content: payload.content,
            flash: payload.flash,
            validity_period: payload.validity_period,
            timeout: payload.timeout,
        };

        let (message_id, response) = state.sms_manager.send_sms(outgoing).await?;
        match response {
            ModemResponse::SendResult(reference_id) => {
                let message_id =
                    message_id.ok_or_else(|| anyhow!("Message sent but no message ID returned"))?;
                Ok(SendSmsResponse {
                    message_id,
                    reference_id,
                })
            }
            ModemResponse::Error(message) => Err(anyhow!(message)),
            _ => Err(anyhow!("Unexpected response type for SMS send request")),
        }
    }
);

http_modem_handler!(sms_get_network_status, ModemRequest::GetNetworkStatus);
http_modem_handler!(sms_get_signal_strength, ModemRequest::GetSignalStrength);
http_modem_handler!(sms_get_network_operator, ModemRequest::GetNetworkOperator);
http_modem_handler!(sms_get_service_provider, ModemRequest::GetServiceProvider);
http_modem_handler!(sms_get_battery_level, ModemRequest::GetBatteryLevel);
http_modem_handler!(gnss_get_status, ModemRequest::GetGNSSStatus);
http_modem_handler!(gnss_get_location, ModemRequest::GetGNSSLocation);

http_get_handler!(sms_get_device_info, SmsDeviceInfo, |state| {
    Ok(SmsDeviceInfo {
        version: crate::VERSION.to_string(),
        phone_number: state.config.phone_number,
        service_provider: modem_extract!(state.sms_manager, ModemRequest::GetServiceProvider => ServiceProvider),
        network_operator: modem_extract!(state.sms_manager, ModemRequest::GetNetworkOperator => NetworkOperator { status, format, operator }),
        network_status: modem_extract!(state.sms_manager, ModemRequest::GetNetworkStatus => NetworkStatus { registration, technology }),
        battery: modem_extract!(state.sms_manager, ModemRequest::GetBatteryLevel => BatteryLevel { status, charge, voltage }),
        signal: modem_extract!(state.sms_manager, ModemRequest::GetSignalStrength => SignalStrength { rssi, ber }),
    })
});

http_get_handler!(sys_version, &'static str, |_state| { Ok(crate::VERSION) });

http_get_handler!(sys_phone_number, Option<String>, |state| {
    Ok(state.config.phone_number)
});

http_post_handler!(
    sys_set_log_level,
    SetLogLevelRequest,
    bool,
    |state, payload| {
        let filter = EnvFilter::from_str(&payload.level)?;
        tracing::log::info!("Setting log level to {filter} via API");

        state
            .tracing_reload
            .reload(filter)
            .map(|_| true)
            .map_err(|e| anyhow!(e))
    }
);

pub async fn websocket_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<HttpState>,
    Query(query_params): Query<WebSocketQuery>,
) -> Result<Response, StatusCode> {
    // Read all target events from query string for filtering.
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
