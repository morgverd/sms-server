use crate::http::routes::*;
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::openapi::{ContentBuilder, RefOr, Response};
use utoipa::Modify;

#[derive(utoipa::OpenApi)]
#[openapi(
    info(
        title = "SMS Server",
    ),
    tags(
        (name = "Database", description = "Database routes"),
        (name = "SMS", description = "SMS sending and device information"),
        (name = "GNSS", description = "GNSS position data"),
        (name = "System", description = "System configuration and status"),
    ),
    paths(
        db_messages,
        db_delivery_reports,
        db_latest_numbers,
        db_friendly_names_set,
        db_friendly_names_get,
        sms_send,
        sms_get_network_status,
        sms_get_signal_strength,
        sms_get_network_operator,
        sms_get_service_provider,
        sms_get_battery_level,
        sms_get_device_info,
        gnss_get_status,
        gnss_get_location,
        sys_phone_number,
        sys_version,
        sys_set_log_level,
        websocket_upgrade
    ),
    modifiers(&OpenApiModifier)
)]
pub struct ApiDoc;

struct OpenApiModifier;
impl Modify for OpenApiModifier {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        // App info
        openapi.info = utoipa::openapi::InfoBuilder::new()
            .title("SMS Server")
            .version(env!("CARGO_PKG_VERSION"))
            .description(Some(env!("CARGO_PKG_DESCRIPTION")))
            .contact(Some(
                utoipa::openapi::ContactBuilder::new()
                    .name(Some(env!("CARGO_PKG_AUTHORS")))
                    .url(Some(env!("CARGO_PKG_HOMEPAGE")))
                    .build(),
            ))
            .license(Some(
                utoipa::openapi::LicenseBuilder::new()
                    .name(env!("CARGO_PKG_LICENSE"))
                    .url(Some(format!(
                        "https://spdx.org/licenses/{}.html",
                        env!("CARGO_PKG_LICENSE")
                    )))
                    .build(),
            ))
            .build();

        // Security scheme
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
            );
        }

        // Global error response examples
        let error_responses = [
            (
                "400",
                "Bad request",
                r#"{"success": false, "error": "Invalid xyz"}"#,
            ),
            (
                "401",
                "Unauthorized",
                r#"{"success": false, "error": "Invalid token"}"#,
            ),
        ];
        for path_item in openapi.paths.paths.values_mut() {
            for op in [&mut path_item.get, &mut path_item.post]
                .into_iter()
                .flatten()
            {
                for (status, desc, _) in error_responses {
                    op.responses
                        .responses
                        .entry(status.to_string())
                        .or_insert_with(|| {
                            let content = ContentBuilder::new()
                                .example(Some(serde_json::json!({
                                    "success": false,
                                    "error": desc
                                })))
                                .build();

                            RefOr::T(
                                Response::builder()
                                    .description(desc)
                                    .content("application/json", content)
                                    .build(),
                            )
                        });
                }
            }
        }
    }
}

pub mod responses {

    macro_rules! create_responses {
        ($($name:ident => $inner:ty),* $(;)?) => {
            $(
                #[derive(serde::Serialize, utoipa::ToSchema)]
                pub struct $name {
                    pub success: bool,
                    pub response: $inner,
                }
            )*
        };
    }

    // Make a concrete type for each response since without it
    // code generation is unusable as everything is inlined.
    // There has to be a better way!
    create_responses! {
        SmsMessagesResponse => Vec<sms_types::sms::SmsMessage>,
        LatestNumbersResponse => Vec<sms_types::http::LatestNumberFriendlyNamePair>,
        DeliveryReportsResponse => Vec<sms_types::sms::SmsDeliveryReport>,
        SmsSendResponse => sms_types::http::HttpSmsSendResponse,
        NetworkStatusResponse => sms_types::http::HttpModemNetworkStatusResponse,
        SignalStrengthResponse => sms_types::http::HttpModemSignalStrengthResponse,
        NetworkOperatorResponse => sms_types::http::HttpModemNetworkOperatorResponse,
        BatteryLevelResponse => sms_types::http::HttpModemBatteryLevelResponse,
        DeviceInfoResponse => sms_types::http::HttpSmsDeviceInfoResponse,
        GnssFixStatusResponse => sms_types::gnss::FixStatus,
        GnssPositionResponse => sms_types::gnss::PositionReport,
        BoolResponse => bool,
        StringResponse => String,
        OptionalStringResponse => Option<String>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoipa::OpenApi;

    #[test]
    fn export_openapi_spec() {
        let spec = ApiDoc::openapi().to_json().unwrap();
        std::fs::write("../../docs/redoc/openapi.json", &spec)
            .expect("Failed to write openapi.json");
        println!("Wrote openapi.json ({} bytes)", spec.len());
    }
}
