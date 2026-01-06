#![cfg_attr(not(feature = "http-server"), allow(dead_code))]

mod database;
mod encryption;
mod multipart;

use crate::config::DatabaseConfig;
use crate::events::{Event, EventBroadcaster};
use crate::modem::sender::ModemSender;
use crate::modem::types::{ModemRequest, ModemResponse};
use crate::sms::database::SMSDatabase;
use crate::sms::multipart::SMSMultipartMessages;
use anyhow::{bail, Result};
use sms_types::sms::{
    SmsIncomingMessage, SmsMessage, SmsOutgoingMessage,
    SmsPartialDeliveryReport,
};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::log::{debug, error, info, warn};

pub type SMSEncryptionKey = [u8; 32];

#[derive(Clone)]
pub struct SMSManager {
    modem: ModemSender,
    database: Arc<SMSDatabase>,
    broadcaster: Option<EventBroadcaster>,
}
impl SMSManager {
    pub async fn connect(
        config: DatabaseConfig,
        modem: ModemSender,
        broadcaster: Option<EventBroadcaster>,
    ) -> Result<Self> {
        let database = Arc::new(SMSDatabase::connect(config).await?);
        Ok(Self {
            modem,
            database,
            broadcaster,
        })
    }

    /// Returns the database row ID and final modem response.
    pub async fn send_sms(
        &self,
        message: SmsOutgoingMessage,
    ) -> Result<(Option<i64>, ModemResponse)> {
        let last_response = match self.modem.send_sms(&message).await? {
            // If all requests were not sent, then don't store any in the database as it must
            // be a failed multipart message. Instead, return the error response.
            (false, Some(response)) => return Ok((None, response)),
            (true, Some(response)) => response,
            _ => bail!("Missing any valid SendSMS response!"),
        };
        debug!("SMSManager last_response: {last_response:?}");

        let mut new_message = SmsMessage::from(&message);
        let send_failure = match &last_response {
            ModemResponse::SendResult(reference_id) => {
                new_message.message_reference.replace(*reference_id);
                None
            }
            ModemResponse::Error(error_message) => {
                new_message.status = None; // TODO: FIX THIS!
                Some(error_message)
            }
            _ => bail!("Got invalid ModemResponse back from sending SMS message!"),
        };

        // Store sent message + send failure in database.
        let message_id_result = match self
            .database
            .insert_message(&new_message, send_failure.is_some())
            .await
        {
            Ok(row_id) => {
                if let Some(failure) = send_failure {
                    if let Err(e) = self.database.insert_send_failure(row_id, failure).await {
                        error!("Failed to store send failure! {e:?}");
                    }
                }
                Ok(row_id)
            }
            Err(e) => Err(e),
        };

        // Broadcast event.
        if let Some(broadcaster) = &self.broadcaster {
            broadcaster
                .broadcast(Event::OutgoingMessage(
                    new_message.with_message_id(message_id_result.as_ref().ok().copied()),
                ))
                .await;
        }

        match message_id_result {
            Ok(message_id) => Ok((Some(message_id), last_response)),
            Err(e) => Err(e),
        }
    }

    pub async fn send_command(&self, request: ModemRequest) -> Result<ModemResponse> {
        self.modem.send_request(request, None).await
    }

    pub fn borrow_database(&self) -> &Arc<SMSDatabase> {
        &self.database
    }
}

/// The multipart key is (phone_number, message_ref), meaning that even if the
/// message reference resets delivery could still work (for unique numbers).
type MultipartReference = (Arc<str>, u8);

#[derive(Clone)]
pub struct SMSReceiver {
    manager: SMSManager,
    multipart: Arc<Mutex<HashMap<MultipartReference, SMSMultipartMessages>>>,
}
impl SMSReceiver {
    pub fn new(manager: SMSManager) -> Self {
        Self {
            manager,
            multipart: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Store + emit incoming SMS message.
    /// Option for multipart messages, as individual parts aren't stored only compiled result.
    pub async fn handle_incoming_sms(
        &mut self,
        incoming_message: SmsIncomingMessage,
    ) -> Option<Result<i64>> {
        // Handle incoming message, discarding if it's a multipart message and not final.
        let message = match self.get_incoming_sms_message(incoming_message).await {
            Some(Ok(message)) => message,
            Some(Err(e)) => return Some(Err(e)),
            None => return None,
        };

        let row_id_result = self.manager.database.insert_message(&message, false).await;

        // Send incoming event.
        if let Some(broadcaster) = &self.manager.broadcaster {
            broadcaster
                .broadcast(Event::IncomingMessage(
                    message.with_message_id(row_id_result.as_ref().ok().copied()),
                ))
                .await;
        }

        Some(row_id_result)
    }

    /// Store + emit delivery report.
    pub async fn handle_delivery_report(&self, report: SmsPartialDeliveryReport) -> Result<i64> {
        // Find the target message from phone number and message reference. This will be fine unless we send 255
        // messages to the client before they reply with delivery reports as then there's no way to properly track.
        let message_id = match self
            .manager
            .database
            .get_delivery_report_target_message(&report.phone_number, report.reference_id)
            .await?
        {
            Some(message_id) => message_id,
            None => bail!("Could not find target message for delivery report!"),
        };

        // Check if we should expect more delivery reports from this message_id.
        // let is_final = report.status.is_success() || report.status.is_permanent_error();
        let is_final = true; /// TODO: ACTUALLY IMPLEMENT THIS!!!
        let status_u8 = report.status as u8;
        info!("IS_FINAL DEBUG TEST LEFT IN!!!!!");

        // Send delivery report event.
        if let Some(broadcaster) = &self.manager.broadcaster {
            broadcaster
                .broadcast(Event::DeliveryReport { message_id, report })
                .await;
        }

        self.manager
            .database
            .insert_delivery_report(message_id, status_u8, is_final)
            .await?;

        self.manager
            .database
            .update_message_status(message_id, status_u8, is_final)
            .await?;

        Ok(message_id)
    }

    /// **Call only from cleanup task!**
    /// Holds multipart lock and removes all stalled receivers.
    pub async fn cleanup_stalled_multipart(&mut self) {
        debug!("Cleaning up stalled multipart messages");
        let mut guard = self.multipart.lock().await;
        guard.retain(|(phone_number, message_reference), messages| {
            // Show a warning whenever a message group has stalled.
            let stalled = messages.is_stalled();
            if stalled {
                warn!(
                    "Removing received multipart message '{phone_number}' (#{message_reference}) has stalled!"
                );
            }
            !stalled
        });
    }

    /// Get the final SMSMessage to broadcast/store, which is either just the
    /// incoming message directly converted or the result of a multipart message compile etc.
    /// Optional result is from the multipart message compile.
    async fn get_incoming_sms_message(
        &mut self,
        incoming_message: SmsIncomingMessage,
    ) -> Option<Result<SmsMessage>> {
        // If there is no multipart header, skip multipart checks.
        let header = match incoming_message.user_data_header.clone() {
            Some(header) => header,
            None => return Some(Ok(SmsMessage::from(&incoming_message))),
        };

        let phone_number: Arc<str> = incoming_message.phone_number.clone().into();
        let multipart_ref: MultipartReference = (phone_number.clone(), header.message_reference);
        debug!("Got multipart reference: {multipart_ref:?}");

        let mut guard = self.multipart.lock().await;
        match guard.entry(multipart_ref) {
            Entry::Vacant(entry) => {
                // New multipart message reference.
                debug!(
                    "Creating new multipart handler, expecting {} parts",
                    header.total
                );
                let mut mulipart = SMSMultipartMessages::with_capacity(header.total as usize);

                if mulipart.add_message(incoming_message, header.index) {
                    warn!("Got a 1 part multipart message from {phone_number}, that's odd!");

                    // Compile message, and don't insert into map since it's complete.
                    Some(mulipart.compile())
                } else {
                    entry.insert(mulipart);
                    None
                }
            }
            Entry::Occupied(mut entry) => {
                // Add message part.
                if entry.get_mut().add_message(incoming_message, header.index) {
                    debug!(
                        "Multipart message complete, compiling {} parts!",
                        header.total
                    );

                    let complete = entry.remove();
                    return Some(complete.compile());
                }

                None
            }
        }
    }
}
