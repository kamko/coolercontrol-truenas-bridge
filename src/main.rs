#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

mod config;
mod service;
mod truenas;

use anyhow::Result;
use clap::Parser;
use config::{SERVICE_ID, load_config};
use log::{LevelFilter, info};
#[cfg(unix)]
use service::TrueNasDeviceService;
use std::str::FromStr;
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(unix)]
use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::SignalKind;
#[cfg(unix)]
use tokio_util::sync::CancellationToken;
#[cfg(unix)]
use tonic::codegen::tokio_stream::wrappers::UnixListenerStream;
#[cfg(unix)]
use tonic::transport::Server;
use truenas::TrueNasClient;

#[cfg(unix)]
use crate::device_service::v1::device_service_server::DeviceServiceServer;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const ENV_CC_LOG: &str = "CC_LOG";

pub mod models {
    pub mod v1 {
        tonic::include_proto!("coolercontrol.models.v1");
    }
}

pub mod device_service {
    pub mod v1 {
        tonic::include_proto!("coolercontrol.device_service.v1");
    }
}

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    #[arg(short, long)]
    debug: bool,

    #[arg(long)]
    config: Option<String>,

    #[arg(long, default_value = "/tmp/coolercontrol-truenas-bridge.sock")]
    socket: String,

    #[arg(long)]
    check: bool,
}

#[cfg(unix)]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    setup_logging(args.debug)?;

    info!("starting {SERVICE_ID} v{VERSION}");
    let config = load_config(args.config.as_deref())?;
    if args.check {
        let client = TrueNasClient::new(config.truenas.clone(), config.polling.connect_timeout());
        let temperatures = client.disk_temperatures().await?;
        println!("{}", serde_json::to_string_pretty(&temperatures)?);
        return Ok(());
    }

    let service = TrueNasDeviceService::new(config);
    let run_token = setup_termination_signals();

    cleanup_uds(&args.socket).await;
    let uds = UnixListener::bind(&args.socket)?;
    let uds_stream = UnixListenerStream::new(uds);

    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<DeviceServiceServer<TrueNasDeviceService>>()
        .await;

    Server::builder()
        .add_service(DeviceServiceServer::new(service))
        .add_service(health_service)
        .serve_with_incoming_shutdown(uds_stream, run_token.cancelled())
        .await?;

    cleanup_uds(&args.socket).await;
    Ok(())
}

#[cfg(not(unix))]
fn main() {
    eprintln!("coolercontrol-truenas-bridge is a Unix-only CoolerControl plugin");
}

fn setup_logging(debug: bool) -> Result<()> {
    let log_level = if debug {
        LevelFilter::Debug
    } else if let Ok(log_lvl) = std::env::var(ENV_CC_LOG) {
        LevelFilter::from_str(&log_lvl).unwrap_or(LevelFilter::Info)
    } else {
        LevelFilter::Info
    };

    env_logger::Builder::new().filter_level(log_level).init();
    Ok(())
}

#[cfg(unix)]
fn setup_termination_signals() -> CancellationToken {
    let run_token = CancellationToken::new();
    let sig_run_token = run_token.clone();

    tokio::task::spawn(async move {
        let ctrl_c = signal::ctrl_c();
        let mut sigterm = signal::unix::signal(SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        let mut sigint = signal::unix::signal(SignalKind::interrupt())
            .expect("failed to install SIGINT handler");
        let mut sigquit =
            signal::unix::signal(SignalKind::quit()).expect("failed to install SIGQUIT handler");

        tokio::select! {
            _ = ctrl_c => {},
            _ = sigterm.recv() => {},
            _ = sigint.recv() => {},
            _ = sigquit.recv() => {},
        }
        sig_run_token.cancel();
        info!("shutting down");
    });

    run_token
}

#[cfg(unix)]
async fn cleanup_uds(uds_path: &str) {
    let _ = tokio::fs::remove_file(uds_path).await;
}
