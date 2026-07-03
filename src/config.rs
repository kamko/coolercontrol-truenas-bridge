use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

pub const SERVICE_ID: &str = env!("CARGO_PKG_NAME");
const DEFAULT_CONFIG_PATH: &str =
    "/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub truenas: TrueNasConfig,
    #[serde(default)]
    pub polling: PollingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrueNasConfig {
    pub host: String,
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_key_file: String,
    #[serde(default = "default_true")]
    pub tls: bool,
    #[serde(default = "default_true")]
    pub tls_verify: bool,
    #[serde(default)]
    pub disk_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollingConfig {
    #[serde(default = "default_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,
    #[serde(default = "default_stale_after_seconds")]
    pub stale_after_seconds: u64,
    #[serde(default = "default_failsafe_temperature_c")]
    pub failsafe_temperature_c: f64,
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: default_poll_interval_seconds(),
            connect_timeout_seconds: default_connect_timeout_seconds(),
            stale_after_seconds: default_stale_after_seconds(),
            failsafe_temperature_c: default_failsafe_temperature_c(),
        }
    }
}

impl PollingConfig {
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.poll_interval_seconds)
    }

    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.connect_timeout_seconds)
    }

    pub fn stale_after(&self) -> Duration {
        Duration::from_secs(self.stale_after_seconds)
    }
}

pub fn load_config(config_path: Option<&str>) -> Result<Config> {
    let path = config_path.unwrap_or(DEFAULT_CONFIG_PATH);
    let data = std::fs::read_to_string(path).with_context(|| format!("read config: {path}"))?;
    let mut config: Config =
        serde_json::from_str(&data).with_context(|| format!("parse config: {path}"))?;

    config.truenas.host = config.truenas.host.trim().to_string();
    config.truenas.endpoint = config.truenas.endpoint.trim().to_string();
    config.truenas.username = config.truenas.username.trim().to_string();
    config.truenas.api_key = config.truenas.api_key.trim().to_string();

    if config.truenas.api_key.is_empty() && !config.truenas.api_key_file.is_empty() {
        config.truenas.api_key = std::fs::read_to_string(Path::new(&config.truenas.api_key_file))
            .with_context(|| format!("read api_key_file: {}", config.truenas.api_key_file))?
            .trim()
            .to_string();
    }

    if config.truenas.host.is_empty() {
        bail!("truenas.host is required");
    }
    if config.truenas.endpoint.is_empty() {
        config.truenas.endpoint = default_endpoint();
    }

    let endpoint = config.truenas.endpoint.clone();
    let normalized_endpoint = endpoint.trim_end_matches('/').to_string();
    if !endpoint.eq_ignore_ascii_case("auto") && !endpoint.starts_with('/') {
        bail!("truenas.endpoint must be \"auto\" or start with /");
    }
    if endpoint.eq_ignore_ascii_case("auto") {
        config.truenas.endpoint = "auto".to_string();
    } else if matches!(normalized_endpoint.as_str(), "/api/current" | "/websocket") {
        config.truenas.endpoint = normalized_endpoint.to_string();
    }
    if config.truenas.api_key.is_empty() {
        bail!("truenas.api_key or truenas.api_key_file is required");
    }
    if normalized_endpoint == "/api/current" && config.truenas.username.is_empty() {
        bail!("truenas.username is required for /api/current API key authentication");
    }
    if config.polling.poll_interval_seconds == 0 {
        bail!("polling.poll_interval_seconds must be at least 1");
    }

    Ok(config)
}

fn default_true() -> bool {
    true
}

fn default_endpoint() -> String {
    "auto".to_string()
}

fn default_poll_interval_seconds() -> u64 {
    300
}

fn default_connect_timeout_seconds() -> u64 {
    15
}

fn default_stale_after_seconds() -> u64 {
    900
}

fn default_failsafe_temperature_c() -> f64 {
    55.0
}
