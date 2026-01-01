#![cfg_attr(not(feature = "http-server"), allow(dead_code))]

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};
use sms_types::sms::{SmsIncomingMessage, SmsPartialDeliveryReport};
use std::fmt::{Display, Formatter};
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum ModemRequest {
    SendSMS { len: usize, pdu: String },
    GetNetworkStatus,
    GetSignalStrength,
    GetNetworkOperator,
    GetServiceProvider,
    GetBatteryLevel,

    // These only work if GNSS is enabled in modem config.
    GetGNSSStatus,
    GetGNSSLocation,
}
impl ModemRequest {
    const TIMEOUT_SMS: Duration = Duration::from_secs(30);
    const TIMEOUT_DEFAULT: Duration = Duration::from_secs(5);

    pub const fn get_default_timeout(&self) -> Duration {
        match self {
            ModemRequest::SendSMS { .. } => Self::TIMEOUT_SMS,
            _ => Self::TIMEOUT_DEFAULT,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ModemResponse {
    SendResult(u8),
    NetworkStatus {
        registration: u8,
        technology: u8,
    },
    SignalStrength {
        rssi: i32,
        ber: i32,
    },
    NetworkOperator {
        status: u8,
        format: u8,
        operator: String,
    },
    ServiceProvider(String),
    BatteryLevel {
        status: u8,
        charge: u8,
        voltage: f32,
    },
    GNSSStatus(GNSSFixStatus),
    GNSSLocation(GNSSLocation),
    Error(String),
}
impl Display for ModemResponse {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ModemResponse::SendResult(reference_id) => write!(f, "SMSResult: Ref {reference_id}"),
            ModemResponse::NetworkStatus {
                registration,
                technology,
            } => write!(f, "NetworkStatus: Reg: {registration}, Tech: {technology}"),
            ModemResponse::SignalStrength { rssi, ber } => {
                write!(f, "SignalStrength: {rssi} dBm ({ber})")
            }
            ModemResponse::NetworkOperator { operator, .. } => {
                write!(f, "NetworkOperator: {operator}")
            }
            ModemResponse::ServiceProvider(operator) => write!(f, "ServiceProvider: {operator}"),
            ModemResponse::BatteryLevel {
                status,
                charge,
                voltage,
            } => write!(
                f,
                "BatteryLevel. Status: {status}, Charge: {charge}, Voltage: {voltage}"
            ),
            ModemResponse::GNSSStatus(status) => write!(f, "GNSS-Status: {status:?}"),
            ModemResponse::GNSSLocation(location) => write!(f, "GNSS-Location: {location:?}"),
            ModemResponse::Error(message) => write!(f, "Error: {message}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModemStatus {
    Startup,
    Online,
    ShuttingDown,
    Offline,
}

#[derive(Debug)]
pub enum ModemEvent {
    UnsolicitedMessage {
        message_type: UnsolicitedMessageType,
        header: String,
    },
    CommandResponse(String),
    Data(String),
    Prompt(String),
}

#[derive(Debug)]
pub enum UnsolicitedMessageType {
    IncomingSMS,
    DeliveryReport,
    NetworkStatusChange,
    ShuttingDown,
    GNSSPositionReport,
}
impl UnsolicitedMessageType {
    pub fn from_header(header: &str) -> Option<Self> {
        if header.starts_with("+CMT") {
            Some(UnsolicitedMessageType::IncomingSMS)
        } else if header.starts_with("+CDS") {
            Some(UnsolicitedMessageType::DeliveryReport)
        } else if header.starts_with("+CGREG:") {
            Some(UnsolicitedMessageType::NetworkStatusChange)
        } else if header.starts_with("+UGNSINF") {
            Some(UnsolicitedMessageType::GNSSPositionReport)
        } else {
            match header {
                "NORMAL POWER DOWN" | "POWER DOWN" | "SHUTDOWN" | "POWERING DOWN" => {
                    Some(UnsolicitedMessageType::ShuttingDown)
                }
                _ => None,
            }
        }
    }

    /// Check if the notification contains additional data on a new line.
    pub fn has_next_line(&self) -> bool {
        match self {
            UnsolicitedMessageType::ShuttingDown => false,
            UnsolicitedMessageType::GNSSPositionReport => false,
            _ => true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ModemIncomingMessage {
    IncomingSMS(SmsIncomingMessage),
    DeliveryReport(SmsPartialDeliveryReport),
    ModemStatusUpdate {
        previous: ModemStatus,
        current: ModemStatus,
    },
    NetworkStatusChange(u8),
    GNSSPositionReport(GNSSLocation),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GNSSFixStatus {
    Unknown,
    NotFix,
    Fix2D,
    Fix3D,
}
impl TryFrom<&str> for GNSSFixStatus {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.trim() {
            "Location Unknown" | "Unknown" => Ok(GNSSFixStatus::Unknown),
            "Location Not Fix" | "Not Fix" => Ok(GNSSFixStatus::NotFix),
            "Location 2D Fix" | "2D Fix" => Ok(GNSSFixStatus::Fix2D),
            "Location 3D Fix" | "3D Fix" => Ok(GNSSFixStatus::Fix3D),
            _ => Err(anyhow!("Invalid GNSS fix status: '{}'", value)),
        }
    }
}
impl From<u8> for GNSSFixStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => GNSSFixStatus::NotFix,
            1 => GNSSFixStatus::Fix2D,
            2 => GNSSFixStatus::Fix3D,
            _ => GNSSFixStatus::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GNSSLocation {
    run_status: bool,
    fix_status: bool,
    utc_time: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    msl_altitude: Option<f64>,
    ground_speed: Option<f32>,
    ground_course: Option<f32>,
    fix_mode: GNSSFixStatus,
    hdop: Option<f32>,
    pdop: Option<f32>,
    vdop: Option<f32>,
    gps_in_view: Option<u8>,
    gnss_used: Option<u8>,
    glonass_in_view: Option<u8>,
}
impl TryFrom<Vec<&str>> for GNSSLocation {
    type Error = anyhow::Error;

    fn try_from(fields: Vec<&str>) -> Result<Self, Self::Error> {
        if fields.len() < 15 {
            bail!("Insufficient GNSS data fields got {}", fields.len());
        }

        // Based on: https://simcom.ee/documents/SIM868/SIM868_GNSS_Application%20Note_V1.00.pdf (2.3)
        Ok(Self {
            run_status: fields[0] == "1",
            fix_status: fields[1] == "1",
            utc_time: fields[2].to_string(),
            latitude: fields[3].parse().ok(),
            longitude: fields[4].parse().ok(),
            msl_altitude: fields[5].parse().ok(),
            ground_speed: fields[6].parse().ok(),
            ground_course: fields[7].parse().ok(),
            fix_mode: GNSSFixStatus::from(fields[8].parse::<u8>().unwrap_or(0)),
            // Reserved1
            hdop: fields[10].parse().ok(),
            pdop: fields[11].parse().ok(),
            vdop: fields[12].parse().ok(),
            // Reserved2
            gps_in_view: fields[14].parse().ok(),
            gnss_used: fields[15].parse().ok(),
            glonass_in_view: fields[16].parse().ok(),
        })
    }
}
impl Display for GNSSLocation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        fn convert_opt<T: Display>(opt: &Option<T>) -> String {
            match opt {
                Some(value) => value.to_string(),
                None => "None".to_string(),
            }
        }

        write!(
            f,
            "Lat: {}, Lon: {}, Alt: {}, Speed: {}, Course: {}",
            convert_opt(&self.latitude),
            convert_opt(&self.longitude),
            convert_opt(&self.msl_altitude),
            convert_opt(&self.ground_speed),
            convert_opt(&self.ground_course)
        )
    }
}
