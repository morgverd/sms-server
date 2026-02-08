use crate::modem::types::{ModemRequest, ModemResponse};
use anyhow::{anyhow, bail, Result};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;
use tracing::log::debug;

static COMMAND_SEQUENCE: AtomicU32 = AtomicU32::new(1);

pub fn next_command_sequence() -> u32 {
    COMMAND_SEQUENCE.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug)]
pub struct CommandContext {
    pub sequence: u32,
    pub state: CommandState,
    pub response_buffer: String,
}

#[derive(Debug, Clone)]
pub enum CommandState {
    WaitingForOk,
    WaitingForPrompt,
    WaitingForData,
}
impl CommandState {
    pub fn is_complete(&self, content: &str) -> bool {
        match self {
            CommandState::WaitingForOk => {
                content == "OK"
                    || content == "ERROR"
                    || content.starts_with("+CME ERROR:")
                    || content.starts_with("+CMS ERROR:")
            }
            CommandState::WaitingForPrompt => false,
            CommandState::WaitingForData => {
                // For SMS, look for the confirmation
                content.starts_with("+CMGS:") || content == "OK" || content == "ERROR"
            }
        }
    }
}

#[derive(Debug)]
pub struct OutgoingCommand {
    pub sequence: u32,
    pub request: ModemRequest,
    timeout: Option<u32>,
    response_tx: Option<oneshot::Sender<ModemResponse>>,
}
impl OutgoingCommand {
    pub fn new(
        sequence: u32,
        response_tx: oneshot::Sender<ModemResponse>,
        request: ModemRequest,
        timeout: Option<u32>,
    ) -> Self {
        Self {
            sequence,
            request,
            timeout,
            response_tx: Some(response_tx),
        }
    }

    /// Get the request specific timeout, this will use whatever is
    /// provided in the response or the base timeout from the ModemRequest.
    pub fn get_request_timeout(&self) -> Duration {
        self.timeout.map_or_else(
            || self.request.get_default_timeout(),
            |t| Duration::from_secs(t as u64),
        )
    }

    /// Respond to the command with a final response.
    pub async fn respond(&mut self, response: ModemResponse) -> Result<()> {
        debug!(
            "Attempting to respond to command #{} with: {:?}",
            self.sequence, response
        );

        if let Some(tx) = self.response_tx.take() {
            debug!(
                "Sending response via oneshot channel for command #{}",
                self.sequence
            );
            match tx.send(response) {
                Ok(_) => {
                    debug!("Successfully sent response for command #{}", self.sequence);
                    Ok(())
                }
                Err(response) => Err(anyhow!(
                    "Failed to respond to command #{} with: {:?}",
                    self.sequence,
                    response
                )),
            }
        } else {
            bail!(
                "Attempted to respond to command #{} but response channel was already used",
                self.sequence
            );
        }
    }
}
