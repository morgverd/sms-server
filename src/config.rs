use anyhow::{Context, Result};
use base64::engine::general_purpose;
use base64::Engine;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use sms_types::events::EventKind;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

#[cfg(feature = "http-server")]
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub database: DatabaseConfig,

    #[cfg(feature = "sentry")]
    pub sentry: Option<SentryConfig>,

    #[serde(default)]
    pub modem: ModemConfig,

    #[cfg(feature = "http-server")]
    #[serde(default)]
    pub http: HTTPConfig,

    #[serde(default)]
    pub webhooks: Option<Vec<ConfiguredWebhook>>,
}
impl AppConfig {
    pub fn load(config_filepath: Option<PathBuf>) -> Result<Self> {
        let config_path = config_filepath.unwrap_or_else(|| PathBuf::from("config.toml"));

        let config_content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file: {config_path:?}"))?;

        let config: AppConfig = toml::from_str(&config_content)
            .with_context(|| format!("Failed to parse TOML config file: {config_path:?}"))?;

        Ok(config)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModemConfig {
    #[serde(default = "default_modem_device")]
    pub device: String,

    #[serde(default = "default_modem_baud")]
    pub baud_rate: u32,

    #[serde(default = "default_false")]
    pub gnss_enabled: bool,

    #[serde(default = "default_gnss_report_interval")]
    pub gnss_report_interval: u32,

    /// The size of Command bounded mpsc sender, should be low. eg: 32
    #[serde(default = "default_modem_cmd_buffer_size")]
    pub cmd_channel_buffer_size: usize,

    #[serde(default = "default_modem_read_buffer_size")]
    pub read_buffer_size: usize,

    #[serde(default = "default_modem_read_buffer_size")]
    pub line_buffer_size: usize,

    #[serde(default = "default_false")]
    #[cfg(feature = "gpio")]
    pub gpio_enabled: bool,

    #[serde(default = "default_gpio_power_pin")]
    #[cfg(feature = "gpio")]
    pub gpio_power_pin: u8,

    #[serde(default = "default_true")]
    #[cfg(feature = "gpio")]
    pub gpio_repower: bool,
}
impl Default for ModemConfig {
    fn default() -> Self {
        Self {
            device: default_modem_device(),
            baud_rate: default_modem_baud(),
            gnss_enabled: default_false(),
            gnss_report_interval: default_gnss_report_interval(),
            cmd_channel_buffer_size: default_modem_cmd_buffer_size(),
            read_buffer_size: default_modem_read_buffer_size(),
            line_buffer_size: default_modem_read_buffer_size(),

            #[cfg(feature = "gpio")]
            gpio_enabled: default_false(),

            #[cfg(feature = "gpio")]
            gpio_power_pin: default_gpio_power_pin(),

            #[cfg(feature = "gpio")]
            gpio_repower: default_true(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub database_url: String,

    #[serde(deserialize_with = "deserialize_encryption_key")]
    pub encryption_key: [u8; 32],
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfiguredWebhook {
    pub url: String,
    pub expected_status: Option<u16>,

    /// By default, this is only IncomingMessage.
    #[serde(default = "default_webhook_events")]
    pub events: Vec<EventKind>,

    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,

    #[serde(deserialize_with = "deserialize_optional_existing_file")]
    #[serde(default)]
    pub certificate_path: Option<PathBuf>,
}
impl ConfiguredWebhook {
    pub fn get_header_map(&self) -> Result<Option<HeaderMap>> {
        let map = if let Some(headers) = &self.headers {
            headers
        } else {
            return Ok(None);
        };

        let mut out = HeaderMap::with_capacity(map.len());
        for (k, v) in map {
            out.insert(
                reqwest::header::HeaderName::from_str(k)?,
                reqwest::header::HeaderValue::from_str(v)?,
            );
        }

        Ok(Some(out))
    }
}

#[cfg(feature = "sentry")]
#[derive(Debug, Deserialize)]
pub struct SentryConfig {
    pub dsn: String,

    #[serde(default)]
    pub environment: Option<String>,

    #[serde(default)]
    pub server_name: Option<String>,

    #[serde(default)]
    pub debug: bool,

    #[serde(default = "default_true")]
    pub send_default_pii: bool,
}

#[cfg(feature = "http-server")]
#[derive(Debug, Clone, Deserialize)]
pub struct HTTPConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_http_address")]
    pub address: SocketAddr,

    #[serde(default = "default_true")]
    pub send_international_format_only: bool,

    #[serde(default = "default_true")]
    pub require_authentication: bool,

    #[serde(default = "default_true")]
    pub websocket_enabled: bool,

    #[serde(default)]
    pub phone_number: Option<String>,

    #[serde(default)]
    pub tls: Option<TLSConfig>,
}
#[cfg(feature = "http-server")]
impl Default for HTTPConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            address: default_http_address(),
            send_international_format_only: default_true(),
            require_authentication: default_true(),
            websocket_enabled: default_true(),
            phone_number: None,
            tls: None,
        }
    }
}
#[cfg_attr(
    not(any(feature = "tls-rustls", feature = "tls-native")),
    allow(dead_code)
)]
#[cfg(feature = "http-server")]
#[derive(Debug, Clone, Deserialize)]
pub struct TLSConfig {
    #[serde(deserialize_with = "deserialize_existing_file")]
    pub certificate_path: PathBuf,

    #[serde(deserialize_with = "deserialize_existing_file")]
    pub key_path: PathBuf,
}

fn default_modem_device() -> String {
    "/dev/ttyS0".to_string()
}
fn default_modem_baud() -> u32 {
    115200
}
fn default_modem_cmd_buffer_size() -> usize {
    32
}
fn default_modem_read_buffer_size() -> usize {
    4096
}
fn default_webhook_events() -> Vec<EventKind> {
    vec![EventKind::IncomingMessage]
}
fn default_gnss_report_interval() -> u32 {
    0
}
fn default_false() -> bool {
    false
}
fn default_true() -> bool {
    true
}

/// Based on the hardware I am using, https://files.waveshare.com/upload/4/4a/GSM_GPRS_GNSS_HAT_User_Manual_EN.pdf
#[cfg(feature = "gpio")]
fn default_gpio_power_pin() -> u8 {
    4
}

#[cfg(feature = "http-server")]
fn default_http_address() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 3000)
}

fn deserialize_encryption_key<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let decoded = general_purpose::STANDARD.decode(&s).map_err(|e| {
        serde::de::Error::custom(format!("Failed to decode base64 encryption key: {e}"))
    })?;

    if decoded.len() != 32 {
        return Err(serde::de::Error::custom(format!(
            "Encryption key must be 32 bytes, got {}",
            decoded.len()
        )));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

fn deserialize_existing_file<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let path = PathBuf::deserialize(deserializer)?;
    if !path.exists() {
        return Err(serde::de::Error::custom(format!(
            "File does not exist: {}",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(serde::de::Error::custom(format!(
            "Path is not a file: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn deserialize_optional_existing_file<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let path_opt = Option::<String>::deserialize(deserializer)?;
    match path_opt {
        Some(path_str) => {
            let path_deserializer = serde::de::value::StringDeserializer::new(path_str);
            Ok(Some(deserialize_existing_file(path_deserializer)?))
        }
        None => Ok(None),
    }
}
