#![cfg_attr(not(feature = "http-server"), allow(dead_code))]

use serde::{Deserialize, Serialize};
use sms_types::gnss::{FixStatus, PositionReport};
use sms_types::modem::ModemStatusUpdateState;
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
    GNSSStatus(FixStatus),
    GNSSLocation(PositionReport),
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
impl From<ModemStatus> for ModemStatusUpdateState {
    /// TODO: Review this, is it required or should ModemStatus be removed entirely?
    ///  It looked weird for the worker to depend on a state from another crate (sms-types)
    fn from(val: ModemStatus) -> Self {
        match val {
            ModemStatus::Startup => ModemStatusUpdateState::Startup,
            ModemStatus::Online => ModemStatusUpdateState::Online,
            ModemStatus::ShuttingDown => ModemStatusUpdateState::ShuttingDown,
            ModemStatus::Offline => ModemStatusUpdateState::Offline,
        }
    }
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
    GNSSPositionReport(PositionReport),
}
