# CoolerControl TrueNAS Bridge

CoolerControl device plugin that exposes TrueNAS disk temperatures as CoolerControl temperature sources.

This is for the HBA-passthrough NAS setup: TrueNAS sees the disks, Proxmox/CoolerControl controls the fans.

## What It Does

- Runs as a CoolerControl device plugin.
- Calls the TrueNAS WebSocket API directly.
- Discovers disks from `disk.temperatures`.
- Exposes each disk as a CoolerControl temperature source.
- Caches temperatures so CoolerControl can poll the plugin frequently without hammering TrueNAS.
- Uses a configurable fail-safe temperature if TrueNAS becomes unavailable.

## TrueNAS Permissions

Create a TrueNAS API key for a service account that can call `disk.temperatures`.

Required role:

```text
REPORTING_READ
```

TrueNAS 25.04 and newer use the JSON-RPC 2.0 WebSocket API. The official TrueNAS client uses `ws(s)://host/api/current` for this API and keeps `ws(s)://host/websocket` for the older legacy protocol. This plugin follows that split: for `/api/current`, it uses `auth.login_ex` with `API_KEY_PLAIN`; for `/websocket`, it uses `auth.login_with_api_key`.

Use HTTPS/WSS for API-key authentication. TrueNAS can revoke API keys used over insecure HTTP, so keep `tls` enabled even when the TrueNAS certificate is self-signed and set `tls_verify` to `false` for local/self-signed certificates.

## Install From Package

Download the `.deb` package from the latest release, then install it on the Proxmox/CoolerControl host:

```bash
cd /tmp
wget https://github.com/kamko/coolercontrol-truenas-bridge/releases/download/v0.1.7/coolercontrol-truenas-bridge_0.1.7_amd64.deb
sudo apt install ./coolercontrol-truenas-bridge_0.1.7_amd64.deb
sudoedit /var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
sudo systemctl restart coolercontrold
```

The package installs:

```text
/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/coolercontrol-truenas-bridge
/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/manifest.toml
/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
```

It creates `config.json` only if it does not already exist.

## Install From Source

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev

git clone https://github.com/kamko/coolercontrol-truenas-bridge.git
cd coolercontrol-truenas-bridge

sudo ./scripts/install-plugin.sh
sudoedit /var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
sudo systemctl restart coolercontrold
```

CoolerControl should then show a `TrueNAS` device with one temperature source per discovered disk.

The plugin manifest runs the service as privileged so the config/API key can stay root-readable only.

## Configuration

Config path:

```text
/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
```

Example:

```json
{
  "truenas": {
    "host": "truenas.local",
    "endpoint": "/api/current",
    "username": "coolercontrol",
    "api_key": "",
    "api_key_file": "/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/api.key",
    "tls": true,
    "tls_verify": false,
    "disk_names": []
  },
  "polling": {
    "poll_interval_seconds": 300,
    "connect_timeout_seconds": 15,
    "stale_after_seconds": 900,
    "failsafe_temperature_c": 55
  }
}
```

`username` is the TrueNAS user that owns the API key. It is required for the `/api/current` endpoint.

`api_key` can be set inline, or left empty when `api_key_file` points to a root-readable file containing only the key.

`disk_names` can stay empty to expose all disks returned by TrueNAS. Set it to a list like `["sda", "sdb"]` to limit the API call.

The plugin uses `disk.query` when available to show richer CoolerControl labels such as `sda - HUH721212AL4200 - SN 12345678`. If the API key cannot call `disk.query`, temperatures still work and labels fall back to raw disk names.

The plugin supports both observed `disk.temperatures` signatures: newer TrueNAS versions using `include_thresholds`, and older versions expecting an `options` object.

`host` can be a bare host such as `truenas.local`, `truenas.local:443`, or a full URL such as `https://truenas.local`. `endpoint` defaults to `/api/current`; older TrueNAS installs may need `/websocket`.

For a local TrueNAS install with a self-signed certificate, the usual settings are:

```json
"tls": true,
"tls_verify": false
```

Test the configured TrueNAS connection manually:

```bash
sudo /var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/coolercontrol-truenas-bridge \
  --config /var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json \
  --check
```

TrueNAS updates disk temperatures roughly every 5 minutes, so `poll_interval_seconds = 300` is the normal default.

## Logs And Troubleshooting

Plugin logs:

```bash
sudo journalctl -u cc-plugin-coolercontrol-truenas-bridge -b -n 200 --no-pager
sudo journalctl -u cc-plugin-coolercontrol-truenas-bridge -b -f
```

Common issues:

- `HTTP 302 Found`: TrueNAS redirected the WebSocket request. The log includes `Location`; set `host` and `endpoint` so the plugin connects directly to that final WebSocket URL.
- `TrueNAS WebSocket closed while waiting for auth.login_ex`: the API key is often invalid or revoked. Regenerate the key after any insecure HTTP test attempt, keep `"tls": true`, and make sure `username` is the user that owns the API key.
- Only `failsafe` appears in CoolerControl: the plugin is running but cannot fetch disk temperatures. Check plugin logs and run `--check`.

## Build

```bash
cargo test
cargo build --release
```

GitHub Actions builds a Linux `amd64` artifact.

Build the Debian package locally:

```bash
cargo build --release
bash scripts/package-deb.sh amd64
```
