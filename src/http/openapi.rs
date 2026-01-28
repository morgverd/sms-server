use crate::http::routes::*;
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::openapi::{ContentBuilder, HttpMethod, RefOr, Response};
use utoipa::Modify;

#[derive(utoipa::OpenApi)]
#[openapi(
    info(
        title = "SMS Server",
    ),
    tags(
        (name = "SMS", description = "SMS sending and device information"),
        (name = "Database", description = "Database queries for messages and delivery reports"),
        (name = "Friendly Names", description = "Contact name management"),
        (name = "System", description = "System configuration and status"),
        (name = "WebSocket", description = "Real-time event streaming")
    ),
    paths(
        // db_sms,
        // db_delivery_reports,
        // db_latest_numbers,
        // friendly_names_set,
        // friendly_names_get,
        // sms_send,
        // sms_get_device_info,
        sys_version,
        sys_phone_number,
        // sys_set_log_level,
        // websocket_upgrade
    ),
    // components(
    //     schemas(
    //         PhoneNumberFetchRequest,
    //         MessageIdFetchRequest,
    //         GlobalFetchRequest,
    //         SetFriendlyNameRequest,
    //         GetFriendlyNameRequest,
    //         SendSmsRequest,
    //         SetLogLevelRequest,
    //         WebSocketQuery,
    //         SendSmsResponse,
    //         SmsDeviceInfo,
    //         SmsMessage,
    //         SmsDeliveryReport,
    //         HttpResponse<()>,
    //     )
    // ),
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
                    .url(Some(format!("https://spdx.org/licenses/{}.html", env!("CARGO_PKG_LICENSE"))))
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
            (
                "500",
                "Internal server error",
                r#"{"success": false, "error": "Internal server error"}"#,
            ),
        ];
        for path_item in openapi.paths.paths.values_mut() {
            for op in [&mut path_item.get, &mut path_item.post]
                .into_iter()
                .flatten()
            {
                for (status, desc, example) in error_responses {
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
