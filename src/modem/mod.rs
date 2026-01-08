use crate::config::{AppConfig, ModemConfig};
use crate::modem::commands::OutgoingCommand;
use crate::modem::sender::ModemSender;
use crate::modem::types::ModemIncomingMessage;
use crate::modem::worker::ModemWorker;
use anyhow::{anyhow, Result};
use tokio::sync::mpsc;
use tokio_serial::SerialPortBuilderExt;
use tracing::log::error;

mod buffer;
mod commands;
mod handlers;
mod parsers;
pub mod sender;
mod state_machine;
pub mod types;
mod worker;

pub struct ModemManager {
    config: ModemConfig,
    main_tx: mpsc::UnboundedSender<ModemIncomingMessage>,
    command_tx: Option<mpsc::Sender<OutgoingCommand>>,
}
impl ModemManager {
    pub fn new(config: &AppConfig) -> (Self, mpsc::UnboundedReceiver<ModemIncomingMessage>) {
        let (main_tx, main_rx) = mpsc::unbounded_channel();
        let manager = Self {
            config: config.modem.clone(),
            main_tx,
            command_tx: None,
        };

        (manager, main_rx)
    }

    pub async fn start(&mut self) -> Result<tokio::task::JoinHandle<()>> {
        let (command_tx, command_rx) = mpsc::channel(self.config.cmd_channel_buffer_size);
        self.command_tx = Some(command_tx);

        let port = tokio_serial::new(&self.config.device, self.config.baud_rate)
            .open_native_async()
            .map_err(|e| anyhow!("Failed to open serial port {}: {}", self.config.device, e))?;

        let worker = ModemWorker::new(port, self.main_tx.clone(), self.config.clone())?;
        let handle = tokio::spawn(async move {
            if let Err(e) = worker.initialize_and_run(command_rx).await {
                error!("ModemWorker error: {e}");
            }
            error!("ModemWorker exit");
        });

        Ok(handle)
    }

    pub fn get_sender(&mut self) -> Result<ModemSender> {
        if let Some(command_tx) = self.command_tx.take() {
            Ok(ModemSender::new(command_tx))
        } else {
            Err(anyhow!("Could not get ModemSender, command_tx channel has already been taken or the modem hasn't been started!"))
        }
    }
}
