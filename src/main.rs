mod app;
mod config;
mod events;
mod modem;
mod sms;
mod webhooks;

#[cfg(feature = "http-server")]
mod http;

use crate::app::AppHandles;
use anyhow::Result;
use clap::Parser;
use dotenv::dotenv;
use std::path::PathBuf;
use tracing::log::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, reload, EnvFilter, Registry};

const VERSION: &str = env!("VERSION");

#[derive(Parser)]
#[command(name = "sms-server")]
#[command(about = env!("CARGO_PKG_DESCRIPTION"))]
#[command(version = VERSION)]
struct CliArguments {
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
}

#[cfg(feature = "sentry")]
fn init_sentry(config: &config::SentryConfig) -> Result<sentry::ClientInitGuard> {
    tracing::log::debug!("Initializing Sentry integration");

    let panic_integration = sentry_panic::PanicIntegration::default().add_extractor(|_| None);
    let guard = sentry::init((
        config.dsn.clone(),
        sentry::ClientOptions {
            environment: config.environment.clone().map(std::borrow::Cow::Owned),
            server_name: config.server_name.clone().map(std::borrow::Cow::Owned),
            debug: config.debug,
            send_default_pii: config.send_default_pii,
            release: sentry::release_name!(),
            integrations: vec![std::sync::Arc::new(panic_integration)],
            before_send: Some(std::sync::Arc::new(|event| {
                tracing::log::warn!(
                    "Sending to Sentry: {}",
                    event
                        .message
                        .as_deref()
                        .or_else(|| {
                            event
                                .exception
                                .values
                                .iter()
                                .filter_map(|e| e.value.as_deref())
                                .next()
                        })
                        .unwrap_or("Unknown!")
                );
                Some(event)
            })),
            ..Default::default()
        },
    ));

    tracing::log::info!("Sentry integration initialized");
    Ok(guard)
}

pub type TracingReloadHandle = reload::Handle<EnvFilter, Registry>;

fn init_tracing() -> TracingReloadHandle {
    let (filter_layer, reload_handle) = reload::Layer::new(EnvFilter::from_default_env());

    let registry = tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt::layer());

    #[cfg(feature = "sentry")]
    let registry = registry.with(sentry_tracing::layer());

    registry.init();
    info!("build version: {VERSION}");

    reload_handle
}

fn main() -> Result<()> {
    dotenv().ok();

    let tracing_reload = init_tracing();
    let args = CliArguments::parse();
    let config = config::AppConfig::load(args.config)?;

    #[cfg(feature = "sentry")]
    let _sentry_guard = config.sentry.as_ref().map(init_sentry).transpose()?;

    #[cfg(not(feature = "sentry"))]
    let _sentry_guard = None;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            let handles = AppHandles::new(config, tracing_reload, _sentry_guard).await?;
            handles.run().await;

            #[cfg(feature = "sentry")]
            {
                tracing::log::info!("Flushing Sentry events before shutdown...");
                if let Some(client) = sentry::Hub::current().client() {
                    client.flush(Some(std::time::Duration::from_secs(5)));
                }
            }

            Ok(())
        })
}
