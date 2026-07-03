use crate::config::TrueNasConfig;
use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use native_tls::TlsConnector;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::Connector;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone)]
pub struct TrueNasClient {
    config: TrueNasConfig,
    timeout: Duration,
}

#[derive(Debug, Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

impl TrueNasClient {
    pub fn new(config: TrueNasConfig, timeout: Duration) -> Self {
        Self { config, timeout }
    }

    pub async fn disk_temperatures(&self) -> Result<BTreeMap<String, f64>> {
        let mut socket = self.connect().await?;
        let mut next_id = 1;

        self.login(&mut socket, &mut next_id).await?;

        let result = self
            .call(
                &mut socket,
                &mut next_id,
                "disk.temperatures",
                json!([self.config.disk_names, false]),
            )
            .await?;

        let raw = result
            .as_object()
            .ok_or_else(|| anyhow!("disk.temperatures returned non-object result"))?;

        let mut temperatures = BTreeMap::new();
        for (disk_name, value) in raw {
            if let Some(temp) = extract_temperature_c(value) {
                temperatures.insert(disk_name.to_string(), temp);
            }
        }

        if temperatures.is_empty() {
            bail!("TrueNAS returned no usable disk temperatures");
        }

        Ok(temperatures)
    }

    async fn connect(&self) -> Result<Socket> {
        let url = self.websocket_url();
        let connector = if self.config.tls && !self.config.tls_verify {
            let tls = TlsConnector::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .context("build insecure TLS connector")?;
            Some(Connector::NativeTls(tls))
        } else {
            None
        };

        let (socket, _) = timeout(
            self.timeout,
            connect_async_tls_with_config(url.as_str(), None, false, connector),
        )
        .await
        .context("connect TrueNAS WebSocket timed out")?
        .with_context(|| format!("connect TrueNAS WebSocket: {url}"))?;

        Ok(socket)
    }

    fn websocket_url(&self) -> String {
        let host = self.config.host.trim_end_matches('/');
        if host.starts_with("ws://") || host.starts_with("wss://") {
            return host.to_string();
        }

        if let Some(rest) = host.strip_prefix("https://") {
            return url_with_default_endpoint("wss", rest, &self.config.endpoint);
        }
        if let Some(rest) = host.strip_prefix("http://") {
            return url_with_default_endpoint("ws", rest, &self.config.endpoint);
        }

        let scheme = if self.config.tls { "wss" } else { "ws" };
        url_with_default_endpoint(scheme, host, &self.config.endpoint)
    }

    async fn login(&self, socket: &mut Socket, next_id: &mut u64) -> Result<()> {
        let result = self
            .call(
                socket,
                next_id,
                "auth.login_with_api_key",
                json!([self.config.api_key]),
            )
            .await?;
        if result.as_bool() != Some(true) {
            bail!("TrueNAS legacy API key authentication failed");
        }
        Ok(())
    }

    async fn call(
        &self,
        socket: &mut Socket,
        next_id: &mut u64,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        let request_id = *next_id;
        *next_id += 1;

        let request = RpcRequest {
            jsonrpc: "2.0",
            id: request_id,
            method,
            params,
        };
        let payload = serde_json::to_string(&request)?;
        timeout(self.timeout, socket.send(Message::Text(payload.into())))
            .await
            .with_context(|| format!("send {method} timed out"))?
            .with_context(|| format!("send {method}"))?;

        loop {
            let message = timeout(self.timeout, socket.next())
                .await
                .with_context(|| format!("read {method} response timed out"))?
                .ok_or_else(|| anyhow!("TrueNAS WebSocket closed while waiting for {method}"))?
                .with_context(|| format!("read {method} response"))?;

            let text = match message {
                Message::Text(text) => text,
                Message::Binary(bytes) => String::from_utf8(bytes.to_vec())
                    .with_context(|| format!("decode binary {method} response"))?
                    .into(),
                _ => continue,
            };
            let response: RpcResponse =
                serde_json::from_str(&text).with_context(|| format!("decode {method} response"))?;

            if response.id != Some(request_id) {
                continue;
            }
            if let Some(error) = response.error {
                bail!("{method} failed: {} ({})", error.message, error.code);
            }
            return response
                .result
                .ok_or_else(|| anyhow!("{method} response had no result"));
        }
    }
}

fn url_with_default_endpoint(scheme: &str, host_or_path: &str, endpoint: &str) -> String {
    if host_or_path.contains('/') {
        format!("{scheme}://{host_or_path}")
    } else {
        format!("{scheme}://{host_or_path}{endpoint}")
    }
}

#[cfg(test)]
mod tests {
    use super::TrueNasClient;
    use crate::config::TrueNasConfig;
    use std::time::Duration;

    fn config(host: &str) -> TrueNasConfig {
        TrueNasConfig {
            host: host.to_string(),
            endpoint: "/api/current".to_string(),
            username: "coolercontrol".to_string(),
            api_key: "key".to_string(),
            api_key_file: String::new(),
            tls: true,
            tls_verify: false,
            disk_names: vec![],
        }
    }

    #[test]
    fn builds_websocket_url_from_bare_host() {
        let client = TrueNasClient::new(config("truenas.local"), Duration::from_secs(1));
        assert_eq!(client.websocket_url(), "wss://truenas.local/api/current");
    }

    #[test]
    fn accepts_https_host() {
        let client = TrueNasClient::new(config("https://truenas.local"), Duration::from_secs(1));
        assert_eq!(client.websocket_url(), "wss://truenas.local/api/current");
    }

    #[test]
    fn preserves_full_websocket_url() {
        let client = TrueNasClient::new(
            config("wss://truenas.local/websocket"),
            Duration::from_secs(1),
        );
        assert_eq!(client.websocket_url(), "wss://truenas.local/websocket");
    }
}

fn extract_temperature_c(value: &Value) -> Option<f64> {
    if let Some(number) = value.as_f64() {
        return Some(number);
    }

    let object = value.as_object()?;
    for key in ["temperature", "temp", "value", "current"] {
        if let Some(nested) = object.get(key).and_then(extract_temperature_c) {
            return Some(nested);
        }
    }
    None
}
