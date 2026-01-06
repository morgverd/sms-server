#![cfg_attr(not(feature = "http-server"), allow(dead_code))]

use crate::modem::commands::{next_command_sequence, OutgoingCommand};
use crate::modem::types::{ModemRequest, ModemResponse};
use anyhow::Result;
use anyhow::{anyhow, bail};
use sms_pdu::pdu::PduAddress;
use sms_pdu::{gsm_encoding, pdu};
use sms_types::sms::SmsOutgoingMessage;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::log::{debug, error, warn};

const SEND_TIMEOUT: Duration = Duration::from_secs(90);

fn create_sms_requests(message: &SmsOutgoingMessage) -> Result<Vec<ModemRequest>> {
    // Parse message number into PduAddress for sending.
    let destination = message
        .to
        .parse::<PduAddress>()
        .map_err(anyhow::Error::msg)?;
    let validity_period = message.get_validity_period();

    let requests = gsm_encoding::GsmMessageData::encode_message(&message.content)
        .into_iter()
        .map(|data| {
            let pdu = pdu::SubmitPdu {
                sca: None,
                first_octet: pdu::PduFirstOctet {
                    mti: pdu::MessageType::SmsSubmit,
                    rd: false,
                    vpf: pdu::VpFieldValidity::Relative,
                    srr: true,
                    udhi: data.udh,
                    rp: false,
                },
                message_id: 0,

                /// TODO: Look into removing this clone.
                destination: destination.clone(),
                dcs: pdu::DataCodingScheme::Standard {
                    compressed: false,
                    class: message
                        .flash
                        .unwrap_or(false)
                        .then_some(pdu::MessageClass::Silent),
                    encoding: data.encoding,
                },
                validity_period,
                user_data: data.bytes,
                user_data_len: data.user_data_len,
            };

            let (bytes, size) = pdu.as_bytes();
            ModemRequest::SendSMS {
                pdu: hex::encode(bytes),
                len: size,
            }
        })
        .collect::<Vec<ModemRequest>>();

    Ok(requests)
}

#[derive(Clone)]
pub struct ModemSender {
    command_tx: mpsc::Sender<OutgoingCommand>,
}
impl ModemSender {
    pub fn new(command_tx: mpsc::Sender<OutgoingCommand>) -> Self {
        Self { command_tx }
    }

    /// Send an SMSOutgoingMessage, and get a resulting ModemResponse.
    /// Returns: Result<(sent_all, Option<last_response>)>
    pub async fn send_sms(
        &self,
        message: &SmsOutgoingMessage,
    ) -> Result<(bool, Option<ModemResponse>)> {
        // Send each send request for message, returning the last message.
        let mut last_response_opt = None;
        for request in create_sms_requests(message)? {
            let response = self.send_request(request, message.timeout).await?;

            // If one of the message parts return an error response, then return immediately
            // as there's no use in continuing to send message parts for a broken concatenation.
            if matches!(response, ModemResponse::Error(_)) {
                return Ok((false, Some(response)));
            }
            last_response_opt.replace(response);
        }

        // Sent all requests, last response
        Ok((true, last_response_opt))
    }

    /// Send a modem request and get some result.
    pub async fn send_request(
        &self,
        request: ModemRequest,
        timeout: Option<u32>,
    ) -> Result<ModemResponse> {
        let sequence = next_command_sequence();
        let (tx, rx) = oneshot::channel();

        debug!("Queuing command sequence {sequence}: {request:?}");
        let cmd = OutgoingCommand::new(sequence, tx, request, timeout);

        // Try to queue without blocking.
        match self.command_tx.try_send(cmd) {
            Ok(_) => debug!("Command sequence {sequence} successfully queued"),
            Err(mpsc::error::TrySendError::Full(_)) => {
                bail!("Command queue is full! The modem may be overwhelmed")
            }
            Err(mpsc::error::TrySendError::Closed(_)) => bail!("Command queue is closed"),
        }

        // Wait for response with timeout.
        let timeout = timeout
            .map(|s| Duration::from_secs(s as u64 + 1))
            .unwrap_or(SEND_TIMEOUT);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => {
                debug!("Command sequence {sequence} completed with response: {response:?}");
                Ok(response)
            }
            Ok(Err(e)) => {
                error!("Command sequence {sequence} response channel error: {e:?}");
                Err(anyhow!(
                    "Command sequence {} response channel closed",
                    sequence
                ))
            }
            Err(_) => {
                warn!("Command sequence {sequence} timed out waiting for response");
                Err(anyhow!(
                    "Command sequence {} timed out waiting for response",
                    sequence
                ))
            }
        }
    }
}
