use crate::config::TrueNasConfig;
use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use log::debug;
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
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Protocol {
    Current,
    Legacy,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct EndpointCandidate {
    endpoint: String,
    protocol: Protocol,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskReading {
    pub temperature_c: f64,
    pub label: String,
}

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

    pub async fn disk_temperatures(&self) -> Result<BTreeMap<String, DiskReading>> {
        let mut errors = Vec::new();

        for candidate in self.endpoint_candidates() {
            match self.disk_temperatures_from_endpoint(&candidate).await {
                Ok(readings) => return Ok(readings),
                Err(err) => {
                    debug!(
                        "TrueNAS endpoint {} failed: {err:#}",
                        candidate.endpoint_label()
                    );
                    errors.push(format!("{}: {err:#}", candidate.endpoint_label()));
                }
            }
        }

        if errors.is_empty() {
            bail!("no TrueNAS endpoint candidates were available");
        }
        bail!(
            "all TrueNAS endpoint candidates failed: {}",
            errors.join("; ")
        )
    }

    async fn disk_temperatures_from_endpoint(
        &self,
        candidate: &EndpointCandidate,
    ) -> Result<BTreeMap<String, DiskReading>> {
        let mut socket = self.connect(candidate).await?;
        let mut next_id = 1;

        self.login(&mut socket, &mut next_id, candidate.protocol)
            .await?;
        let result = self
            .disk_temperatures_call(&mut socket, &mut next_id, candidate.protocol)
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

        let labels = match self
            .disk_labels(&mut socket, &mut next_id, candidate.protocol)
            .await
        {
            Ok(labels) => labels,
            Err(err) => {
                debug!("failed to fetch TrueNAS disk labels: {err:#}");
                BTreeMap::new()
            }
        };

        Ok(temperatures
            .into_iter()
            .map(|(disk_name, temperature_c)| {
                let label = labels
                    .get(&disk_name)
                    .cloned()
                    .unwrap_or_else(|| disk_name.clone());
                (
                    disk_name,
                    DiskReading {
                        temperature_c,
                        label,
                    },
                )
            })
            .collect())
    }

    async fn connect(&self, candidate: &EndpointCandidate) -> Result<Socket> {
        let url = self.websocket_url(candidate);
        let connector = if self.config.tls && !self.config.tls_verify {
            let tls = TlsConnector::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .context("build insecure TLS connector")?;
            Some(Connector::NativeTls(tls))
        } else {
            None
        };

        let connect_result = timeout(
            self.timeout,
            connect_async_tls_with_config(url.as_str(), None, false, connector),
        )
        .await
        .context("connect TrueNAS WebSocket timed out")?;

        match connect_result {
            Ok((socket, _)) => Ok(socket),
            Err(err) => bail!("{}", describe_connect_error(&url, err)),
        }
    }

    fn websocket_url(&self, candidate: &EndpointCandidate) -> String {
        let host = self.config.host.trim_end_matches('/');
        if host.starts_with("ws://") || host.starts_with("wss://") {
            return host.to_string();
        }

        if let Some(rest) = host.strip_prefix("https://") {
            return url_with_endpoint("wss", rest, &candidate.endpoint);
        }
        if let Some(rest) = host.strip_prefix("http://") {
            return url_with_endpoint("ws", rest, &candidate.endpoint);
        }

        let scheme = if self.config.tls { "wss" } else { "ws" };
        url_with_endpoint(scheme, host, &candidate.endpoint)
    }

    fn endpoint_candidates(&self) -> Vec<EndpointCandidate> {
        let endpoint = self.config.endpoint.trim();
        let host = self.config.host.trim_end_matches('/');

        if host.starts_with("ws://") || host.starts_with("wss://") {
            return vec![EndpointCandidate {
                endpoint: String::new(),
                protocol: protocol_for_endpoint(host),
            }];
        }

        if endpoint.eq_ignore_ascii_case("auto") {
            let mut candidates = Vec::new();
            if !self.config.username.is_empty() {
                candidates.push(EndpointCandidate::current());
            }
            candidates.push(EndpointCandidate::legacy());
            return candidates;
        }

        vec![EndpointCandidate {
            endpoint: endpoint.to_string(),
            protocol: protocol_for_endpoint(endpoint),
        }]
    }

    async fn login(
        &self,
        socket: &mut Socket,
        next_id: &mut u64,
        protocol: Protocol,
    ) -> Result<()> {
        match protocol {
            Protocol::Current => {
                let result = self
                    .call(
                        socket,
                        next_id,
                        "auth.login_ex",
                        json!([{
                            "mechanism": "API_KEY_PLAIN",
                            "username": self.config.username,
                            "api_key": self.config.api_key
                        }]),
                    )
                    .await?;

                match result.get("response_type").and_then(Value::as_str) {
                    Some("SUCCESS") => Ok(()),
                    Some("AUTH_ERR") => bail!("TrueNAS API key authentication failed"),
                    Some("EXPIRED") => bail!("TrueNAS API key has expired"),
                    Some("DENIED") => bail!("TrueNAS account does not have API access"),
                    Some("REDIRECT") => bail!("TrueNAS authentication returned redirect: {result}"),
                    _ => bail!("TrueNAS auth.login_ex returned unexpected response: {result}"),
                }
            }
            Protocol::Legacy => {
                self.legacy_handshake(socket).await?;
                let result = self
                    .legacy_call(
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
        }
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

            let Some(text) = message_to_text(message, method)? else {
                continue;
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

    async fn api_call(
        &self,
        socket: &mut Socket,
        next_id: &mut u64,
        method: &str,
        params: Value,
        protocol: Protocol,
    ) -> Result<Value> {
        match protocol {
            Protocol::Current => self.call(socket, next_id, method, params).await,
            Protocol::Legacy => self.legacy_call(socket, next_id, method, params).await,
        }
    }

    async fn disk_temperatures_call(
        &self,
        socket: &mut Socket,
        next_id: &mut u64,
        protocol: Protocol,
    ) -> Result<Value> {
        let current_params = json!([self.config.disk_names, false]);
        match self
            .api_call(
                socket,
                next_id,
                "disk.temperatures",
                current_params,
                protocol,
            )
            .await
        {
            Ok(result) => Ok(result),
            Err(err) if should_retry_disk_temperature_options(&err) => {
                self.api_call(
                    socket,
                    next_id,
                    "disk.temperatures",
                    json!([self.config.disk_names, {}]),
                    protocol,
                )
                .await
            }
            Err(err) => Err(err),
        }
    }

    async fn disk_labels(
        &self,
        socket: &mut Socket,
        next_id: &mut u64,
        protocol: Protocol,
    ) -> Result<BTreeMap<String, String>> {
        let rows = self
            .api_call(socket, next_id, "disk.query", json!([[], {}]), protocol)
            .await?;
        let rows = rows
            .as_array()
            .ok_or_else(|| anyhow!("disk.query returned non-array result"))?;

        let mut labels = BTreeMap::new();
        for row in rows {
            let Some(name) = row.get("name").and_then(Value::as_str) else {
                continue;
            };
            labels.insert(name.to_string(), disk_label(row, name));
        }
        Ok(labels)
    }

    async fn legacy_handshake(&self, socket: &mut Socket) -> Result<()> {
        let payload = json!({
            "msg": "connect",
            "version": "1",
            "support": ["1"]
        });
        timeout(
            self.timeout,
            socket.send(Message::Text(payload.to_string().into())),
        )
        .await
        .context("send legacy connect timed out")?
        .context("send legacy connect")?;

        loop {
            let message = timeout(self.timeout, socket.next())
                .await
                .context("read legacy connect response timed out")?
                .ok_or_else(|| anyhow!("TrueNAS WebSocket closed during legacy connect"))?
                .context("read legacy connect response")?;

            let Some(text) = message_to_text(message, "legacy connect")? else {
                continue;
            };
            let response: Value =
                serde_json::from_str(&text).context("decode legacy connect response")?;
            match response.get("msg").and_then(Value::as_str) {
                Some("connected") => return Ok(()),
                Some("failed") => bail!("TrueNAS legacy WebSocket rejected protocol: {response}"),
                _ => continue,
            }
        }
    }

    async fn legacy_call(
        &self,
        socket: &mut Socket,
        next_id: &mut u64,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        let request_id = next_id.to_string();
        *next_id += 1;

        let payload = json!({
            "msg": "method",
            "method": method,
            "id": request_id,
            "params": params
        });
        timeout(
            self.timeout,
            socket.send(Message::Text(payload.to_string().into())),
        )
        .await
        .with_context(|| format!("send {method} timed out"))?
        .with_context(|| format!("send {method}"))?;

        loop {
            let message = timeout(self.timeout, socket.next())
                .await
                .with_context(|| format!("read {method} response timed out"))?
                .ok_or_else(|| anyhow!("TrueNAS WebSocket closed while waiting for {method}"))?
                .with_context(|| format!("read {method} response"))?;

            let Some(text) = message_to_text(message, method)? else {
                continue;
            };
            let response: Value =
                serde_json::from_str(&text).with_context(|| format!("decode {method} response"))?;
            if response.get("id").and_then(Value::as_str) != Some(request_id.as_str()) {
                continue;
            }
            if response.get("msg").and_then(Value::as_str) != Some("result") {
                continue;
            }
            if let Some(error) = response.get("error") {
                bail!("{method} failed: {error}");
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| anyhow!("{method} response had no result"));
        }
    }
}

fn message_to_text(message: Message, method: &str) -> Result<Option<String>> {
    match message {
        Message::Text(text) => Ok(Some(text.to_string())),
        Message::Binary(bytes) => String::from_utf8(bytes.to_vec())
            .with_context(|| format!("decode binary {method} response"))
            .map(Some),
        Message::Close(Some(frame)) => bail!(
            "TrueNAS WebSocket closed while waiting for {method}: {} {}",
            frame.code,
            frame.reason
        ),
        Message::Close(None) => bail!("TrueNAS WebSocket closed while waiting for {method}"),
        _ => Ok(None),
    }
}

fn should_retry_disk_temperature_options(err: &anyhow::Error) -> bool {
    let message = format!("{err:#}");
    message.contains("options") && message.contains("A dict was expected")
}

fn describe_connect_error(url: &str, err: WsError) -> String {
    match err {
        WsError::Http(response) => {
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("<missing>");
            format!(
                "connect TrueNAS WebSocket: {url}: HTTP {}; Location: {location}",
                response.status()
            )
        }
        other => format!("connect TrueNAS WebSocket: {url}: {other}"),
    }
}

fn disk_label(row: &Value, name: &str) -> String {
    let description = row.get("description").and_then(non_empty_string);
    let model = row.get("model").and_then(non_empty_string);
    let serial = row.get("serial").and_then(non_empty_string);

    let mut parts = vec![name.to_string()];
    if let Some(description) = description {
        parts.push(description.to_string());
    } else if let Some(model) = model {
        parts.push(model.to_string());
    }
    if let Some(serial) = serial {
        parts.push(format!("SN {}", compact_serial(serial)));
    }

    parts.join(" - ")
}

fn non_empty_string(value: &Value) -> Option<&str> {
    let text = value.as_str()?.trim();
    if text.is_empty() { None } else { Some(text) }
}

fn compact_serial(serial: &str) -> String {
    const SERIAL_TAIL_LENGTH: usize = 8;
    let serial = serial.trim();
    let chars = serial.chars().collect::<Vec<_>>();
    if chars.len() <= SERIAL_TAIL_LENGTH {
        serial.to_string()
    } else {
        chars[chars.len() - SERIAL_TAIL_LENGTH..].iter().collect()
    }
}

impl EndpointCandidate {
    fn current() -> Self {
        Self {
            endpoint: "/api/current".to_string(),
            protocol: Protocol::Current,
        }
    }

    fn legacy() -> Self {
        Self {
            endpoint: "/websocket".to_string(),
            protocol: Protocol::Legacy,
        }
    }

    fn endpoint_label(&self) -> &str {
        if self.endpoint.is_empty() {
            "<host URL>"
        } else {
            &self.endpoint
        }
    }
}

fn protocol_for_endpoint(endpoint: &str) -> Protocol {
    if endpoint.trim_end_matches('/').ends_with("/websocket") {
        Protocol::Legacy
    } else {
        Protocol::Current
    }
}

fn url_with_endpoint(scheme: &str, host_or_path: &str, endpoint: &str) -> String {
    if host_or_path.contains('/') {
        format!("{scheme}://{host_or_path}")
    } else {
        format!("{scheme}://{host_or_path}{endpoint}")
    }
}

#[cfg(test)]
mod tests {
    use super::{EndpointCandidate, Protocol, TrueNasClient, compact_serial, disk_label};
    use crate::config::TrueNasConfig;
    use serde_json::json;
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
        assert_eq!(
            client.websocket_url(&EndpointCandidate::current()),
            "wss://truenas.local/api/current"
        );
    }

    #[test]
    fn accepts_https_host() {
        let client = TrueNasClient::new(config("https://truenas.local"), Duration::from_secs(1));
        assert_eq!(
            client.websocket_url(&EndpointCandidate::current()),
            "wss://truenas.local/api/current"
        );
    }

    #[test]
    fn preserves_full_websocket_url() {
        let client = TrueNasClient::new(
            config("wss://truenas.local/websocket"),
            Duration::from_secs(1),
        );
        assert_eq!(
            client.websocket_url(&EndpointCandidate::legacy()),
            "wss://truenas.local/websocket"
        );
    }

    #[test]
    fn auto_endpoint_tries_current_then_legacy_with_username() {
        let mut config = config("truenas.local");
        config.endpoint = "auto".to_string();
        let client = TrueNasClient::new(config, Duration::from_secs(1));

        assert_eq!(
            client.endpoint_candidates(),
            vec![EndpointCandidate::current(), EndpointCandidate::legacy()]
        );
    }

    #[test]
    fn auto_endpoint_skips_current_without_username() {
        let mut config = config("truenas.local");
        config.endpoint = "auto".to_string();
        config.username = String::new();
        let client = TrueNasClient::new(config, Duration::from_secs(1));

        assert_eq!(
            client.endpoint_candidates(),
            vec![EndpointCandidate::legacy()]
        );
    }

    #[test]
    fn websocket_url_host_forces_legacy_protocol() {
        let client = TrueNasClient::new(
            config("wss://truenas.local/websocket"),
            Duration::from_secs(1),
        );

        assert_eq!(
            client.endpoint_candidates(),
            vec![EndpointCandidate {
                endpoint: String::new(),
                protocol: Protocol::Legacy,
            }]
        );
    }

    #[test]
    fn trailing_slash_websocket_endpoint_is_legacy() {
        let mut config = config("truenas.local");
        config.endpoint = "/websocket/".to_string();
        let client = TrueNasClient::new(config, Duration::from_secs(1));

        assert_eq!(
            client.endpoint_candidates(),
            vec![EndpointCandidate {
                endpoint: "/websocket/".to_string(),
                protocol: Protocol::Legacy,
            }]
        );
    }

    #[test]
    fn builds_readable_disk_label() {
        let row = json!({
            "name": "sda",
            "model": "HUH721212AL4200",
            "serial": "ABCDEFGH12345678"
        });

        assert_eq!(
            disk_label(&row, "sda"),
            "sda - HUH721212AL4200 - SN 12345678"
        );
    }

    #[test]
    fn description_takes_precedence_in_disk_label() {
        let row = json!({
            "name": "sda",
            "description": "front bay 1",
            "model": "HUH721212AL4200",
            "serial": "1234"
        });

        assert_eq!(disk_label(&row, "sda"), "sda - front bay 1 - SN 1234");
    }

    #[test]
    fn compacts_long_serials() {
        assert_eq!(compact_serial("ABCDEFGH12345678"), "12345678");
        assert_eq!(compact_serial("1234"), "1234");
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
