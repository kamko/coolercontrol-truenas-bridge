# CoolerControl TrueNAS Bridge

[![CI](https://github.com/kamko/coolercontrol-truenas-bridge/actions/workflows/ci.yml/badge.svg)](https://github.com/kamko/coolercontrol-truenas-bridge/actions/workflows/ci.yml)

Expose TrueNAS disk temperatures as CoolerControl temperature sensors.

This is useful for home NAS and homelab setups where disks are attached to a TrueNAS VM through HBA passthrough, while the host that controls the fans runs Proxmox, Linux, and CoolerControl. TrueNAS can see the HDD temperatures; CoolerControl can control the fans. This plugin bridges the gap so you can build fan curves from TrueNAS disk temperatures.

Keywords: CoolerControl plugin, TrueNAS disk temperature, Proxmox fan control, HBA passthrough, NAS HDD cooling, homelab fan curve.

## Features

- Runs as a native CoolerControl device plugin.
- Calls the TrueNAS WebSocket API directly; no Prometheus, database, or sidecar service required.
- Exposes one CoolerControl temperature source per discovered disk.
- Uses richer labels from TrueNAS when available, such as `sda - HUH721212AL4200 - SN 12345678`.
- Supports modern `/api/current` JSON-RPC and legacy `/websocket` TrueNAS endpoints.
- Auto-detects the endpoint by trying `/api/current` first when a username is configured, then falling back to `/websocket`.
- Caches temperatures to avoid hammering TrueNAS.
- Uses configurable fail-safe temperatures if TrueNAS becomes unavailable.
- Ships Linux `amd64` `.deb` packages from GitHub Releases.

## Requirements

- CoolerControl installed on the Linux host that controls the fans.
- Network access from that host to the TrueNAS UI/API address.
- A TrueNAS API key.

Create the API key for a service account that can call:

- `disk.temperatures` for temperatures.
- `disk.query` for nicer labels. This is optional; if denied, labels fall back to `sda`, `sdb`, etc.

For minimal TrueNAS roles, start with `REPORTING_READ` for temperatures and add disk read permissions if richer labels are not shown.

Use HTTPS/WSS for API-key authentication. TrueNAS may revoke API keys used over insecure HTTP, so keep `tls` enabled even when the TrueNAS certificate is self-signed and set `tls_verify` to `false` for local certificates.

## Install

Download and install the latest Debian package on the Proxmox/CoolerControl host:

```bash
cd /tmp
wget https://github.com/kamko/coolercontrol-truenas-bridge/releases/download/v0.1.8/coolercontrol-truenas-bridge_0.1.8_amd64.deb
sudo apt install ./coolercontrol-truenas-bridge_0.1.8_amd64.deb
sudoedit /var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
sudo systemctl restart coolercontrold
```

The package installs:

```text
/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/coolercontrol-truenas-bridge
/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/manifest.toml
/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
```

`config.json` is created only if it does not already exist, so package upgrades preserve your API key and local settings.

## Configuration

Config path:

```text
/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
```

Recommended config:

```json
{
  "truenas": {
    "host": "192.168.100.12",
    "endpoint": "auto",
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

`host` can be a bare host/IP such as `192.168.100.12`, `truenas.local`, `truenas.local:443`, or a full URL such as `https://truenas.local`.

`endpoint` should normally stay `auto`, and you can omit it from config entirely because `auto` is the default. In auto mode, the plugin tries:

1. `/api/current` with `auth.login_ex` and `API_KEY_PLAIN` when `username` is set.
2. `/websocket` with legacy `auth.login_with_api_key`.

Set `endpoint` manually only when you want to pin behavior:

```json
"endpoint": "/api/current"
```

or:

```json
"endpoint": "/websocket"
```

`username` is the TrueNAS user that owns the API key. It is required only for `/api/current`; legacy `/websocket` can work without it. Keeping it configured is recommended.

`api_key` can be set inline, but `api_key_file` is cleaner. The file should contain only the API key and be readable by root.

`disk_names` can stay empty to expose every disk returned by TrueNAS. Set it to a list like `["sda", "sdb"]` to limit the API call.

The plugin uses `disk.query` when available to show richer CoolerControl labels such as `sda - HUH721212AL4200 - SN 12345678`. If the API key cannot call `disk.query`, temperatures still work and labels fall back to raw disk names.

TrueNAS updates disk temperatures roughly every 5 minutes, so `poll_interval_seconds = 300` is the normal default.

## Test

Run the plugin check command directly:

```bash
sudo /var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/coolercontrol-truenas-bridge \
  --config /var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json \
  --check
```

Expected output is JSON keyed by disk name. Each disk includes the temperature and display label.

## Logs

Plugin logs:

```bash
sudo journalctl -u cc-plugin-coolercontrol-truenas-bridge -b -n 200 --no-pager
sudo journalctl -u cc-plugin-coolercontrol-truenas-bridge -b -f
```

CoolerControl daemon logs:

```bash
sudo journalctl -u coolercontrold -b -n 200 --no-pager
```

## Troubleshooting

Only `failsafe` appears in CoolerControl:

The plugin is running, but it cannot fetch TrueNAS temperatures. Run `--check` and inspect the plugin journal.

`HTTP 302 Found` or `Location: .../ui/`:

Your TrueNAS did not accept the attempted WebSocket endpoint. Leave `endpoint` as `auto`, or pin `/websocket` if your TrueNAS only exposes the legacy endpoint.

`TrueNAS WebSocket closed while waiting for auth.login_ex`:

Regenerate the API key, keep `tls: true`, keep `tls_verify: false` for self-signed local certificates, and ensure `username` matches the API key owner.

Disk temperatures work, but labels are still only `sda`, `sdb`, etc.:

The API key probably cannot call `disk.query`. Add disk read permission, or set disk descriptions in TrueNAS and allow `disk.query`.

`0.0 C` for a disk:

That value came from TrueNAS. Some disks, controllers, or sleeping drives may not report SMART temperature consistently.

## How It Works

CoolerControl loads the plugin from:

```text
/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/
```

The plugin registers a single CoolerControl device named `TrueNAS`. Every TrueNAS disk temperature becomes a CoolerControl temperature channel. You can then use those temperatures in CoolerControl fan profiles exactly like local motherboard or drive sensors.

TrueNAS API compatibility:

- `/api/current`: JSON-RPC 2.0 WebSocket endpoint used by newer TrueNAS versions.
- `/websocket`: legacy WebSocket endpoint used by older TrueNAS versions.

The plugin supports both observed `disk.temperatures` signatures: versions using `include_thresholds` and versions expecting an `options` object.

## Build

```bash
cargo test
cargo build --release
```

Install from source:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev
sudo ./scripts/install-plugin.sh
sudoedit /var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
sudo systemctl restart coolercontrold
```

Build a Debian package locally:

```bash
cargo build --release
bash scripts/package-deb.sh amd64
```

GitHub Actions builds and publishes Linux `amd64` release artifacts for tagged versions.
