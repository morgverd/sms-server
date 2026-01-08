use crate::modem::commands::CommandState;
use crate::modem::parsers::*;
use crate::modem::types::{
    ModemIncomingMessage, ModemRequest, ModemResponse, ModemStatus, UnsolicitedMessageType,
};
use crate::modem::worker::WorkerEvent;
use anyhow::{anyhow, bail, Result};
use sms_pdu::pdu::{DeliverPdu, StatusReportPdu};
use sms_types::sms::{SmsIncomingMessage, SmsMultipartHeader, SmsPartialDeliveryReport};
use tokio::sync::mpsc;
use tracing::log::{debug, warn};

/// Invoked early by receivers to handle an edge case where certain carriers respond
/// to automated test numbers using an alphanumeric sender ID instead of a numeric one.
fn get_real_number(phone_number: String) -> String {
    match phone_number.as_str() {
        "ASDAmobile" => "2732".to_string(),
        _ => phone_number,
    }
}

pub struct ModemEventHandlers {
    worker_event_tx: mpsc::UnboundedSender<WorkerEvent>,
}
impl ModemEventHandlers {
    pub fn new(worker_event_tx: mpsc::UnboundedSender<WorkerEvent>) -> Self {
        Self { worker_event_tx }
    }

    pub async fn command_sender(&self, request: &ModemRequest) -> Result<CommandState> {
        match request {
            ModemRequest::SendSMS { len, .. } => {
                let command = format!("AT+CMGS={len}\r\n");
                self.write(command.as_bytes()).await?;
                return Ok(CommandState::WaitingForPrompt);
            }
            ModemRequest::GetNetworkStatus => self.write(b"AT+CREG?\r\n").await?,
            ModemRequest::GetSignalStrength => self.write(b"AT+CSQ\r\n").await?,
            ModemRequest::GetNetworkOperator => self.write(b"AT+COPS?\r\n").await?,
            ModemRequest::GetServiceProvider => self.write(b"AT+CSPN?\r\n").await?,
            ModemRequest::GetBatteryLevel => self.write(b"AT+CBC\r\n").await?,
            ModemRequest::GetGNSSStatus => self.write(b"AT+CGPSSTATUS?\r\n").await?,
            ModemRequest::GetGNSSLocation => self.write(b"AT+CGNSINF\r\n").await?,
        }
        Ok(CommandState::WaitingForData)
    }

    pub async fn prompt_handler(&self, request: &ModemRequest) -> Result<Option<CommandState>> {
        if let ModemRequest::SendSMS { len, pdu } = request {
            debug!("Sending PDU: len = {len}");

            // Push CTRL+Z to end of PDU to submit.
            let encoded = pdu.as_bytes();
            let mut buf = Vec::with_capacity(encoded.len() + 1);
            buf.extend_from_slice(encoded);
            buf.push(0x1A);
            self.write(&buf).await?;

            return Ok(Some(CommandState::WaitingForOk));
        }

        Ok(None)
    }

    pub async fn handle_unsolicited_message(
        &self,
        message_type: &UnsolicitedMessageType,
        content: &str,
    ) -> Result<Option<ModemIncomingMessage>> {
        debug!("UnsolicitedMessage: {:?} -> {:?}", &message_type, &content);

        match message_type {
            UnsolicitedMessageType::IncomingSMS => {
                let content_hex = hex::decode(content).map_err(|e| anyhow!(e))?;
                let deliver_pdu =
                    DeliverPdu::try_from(content_hex.as_slice()).map_err(|e| anyhow!(e))?;

                // Decode incoming message data to get user data header which is required for multipart messages.
                let incoming = match deliver_pdu.get_message_data().decode_message() {
                    Ok(msg) => SmsIncomingMessage {
                        phone_number: get_real_number(deliver_pdu.originating_address.to_string()),
                        user_data_header: msg
                            .udh
                            .map(|udh| SmsMultipartHeader::try_from(udh.as_bytes()))
                            .transpose()
                            .map_err(anyhow::Error::msg)?,
                        content: msg.text,
                    },
                    Err(e) => bail!("Failed to parse incoming SMS data: {:?}", e),
                };

                Ok(Some(ModemIncomingMessage::IncomingSMS(incoming)))
            }
            UnsolicitedMessageType::DeliveryReport => {
                let content_hex = hex::decode(content).map_err(|e| anyhow!(e))?;
                let status_report_pdu =
                    StatusReportPdu::try_from(content_hex.as_slice()).map_err(|e| anyhow!(e))?;

                let report = SmsPartialDeliveryReport {
                    status: status_report_pdu.status as u8,
                    phone_number: get_real_number(status_report_pdu.recipient_address.to_string()),
                    reference_id: status_report_pdu.message_reference,
                };
                Ok(Some(ModemIncomingMessage::DeliveryReport(report)))
            }
            UnsolicitedMessageType::NetworkStatusChange => {
                Ok(Some(ModemIncomingMessage::NetworkStatusChange(0)))
            }
            UnsolicitedMessageType::ShuttingDown => {
                warn!("The modem is shutting down!");
                self.set_status(ModemStatus::ShuttingDown).await?;
                Ok(None)
            }
            UnsolicitedMessageType::GNSSPositionReport => Ok(Some(
                ModemIncomingMessage::GNSSPositionReport(parse_cgnsinf_response(content, true)?),
            )),
        }
    }

    pub async fn command_responder(
        &self,
        request: &ModemRequest,
        response: &String,
    ) -> Result<ModemResponse> {
        debug!("Command response: {request:?} -> {response:?}");
        if !response.trim_end().ends_with("OK") {
            return Err(anyhow!("Modem response does not end with OK"));
        }

        match request {
            ModemRequest::SendSMS { .. } => {
                Ok(ModemResponse::SendResult(parse_cmgs_result(response)?))
            }
            ModemRequest::GetNetworkStatus => {
                let (registration, technology) = parse_creg_response(response)?;
                Ok(ModemResponse::NetworkStatus {
                    registration,
                    technology,
                })
            }
            ModemRequest::GetSignalStrength => {
                let (rssi, ber) = parse_csq_response(response)?;
                Ok(ModemResponse::SignalStrength { rssi, ber })
            }
            ModemRequest::GetNetworkOperator => {
                let (status, format, operator) = parse_cops_response(response)?;
                Ok(ModemResponse::NetworkOperator {
                    status,
                    format,
                    operator,
                })
            }
            ModemRequest::GetServiceProvider => Ok(ModemResponse::ServiceProvider(
                parse_cspn_response(response)?,
            )),
            ModemRequest::GetBatteryLevel => {
                let (status, charge, voltage) = parse_cbc_response(response)?;
                Ok(ModemResponse::BatteryLevel {
                    status,
                    charge,
                    voltage,
                })
            }
            ModemRequest::GetGNSSStatus => Ok(ModemResponse::GNSSStatus(
                parse_cgpsstatus_response(response)?,
            )),
            ModemRequest::GetGNSSLocation => Ok(ModemResponse::GNSSLocation(
                parse_cgnsinf_response(response, false)?,
            )),
        }
    }

    async fn write(&self, data: &[u8]) -> Result<()> {
        self.worker_event_tx
            .send(WorkerEvent::WriteCommand(data.to_vec()))
            .map_err(|_| anyhow!("Failed to send write command event"))?;
        Ok(())
    }

    async fn set_status(&self, status: ModemStatus) -> Result<()> {
        self.worker_event_tx
            .send(WorkerEvent::SetStatus(status))
            .map_err(|_| anyhow!("Failed to send status change event"))?;
        Ok(())
    }
}
