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

TrueNAS 25.04 and newer use the WebSocket API. This plugin uses `auth.login_ex` with `API_KEY_PLAIN`.

## Install From Package

Download the `.deb` artifact from the latest successful GitHub Actions run or from a tagged release, then install it on the Proxmox/CoolerControl host:

```bash
sudo apt install ./coolercontrol-truenas-bridge_*_amd64.deb
sudoedit /etc/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
sudo systemctl restart coolercontrold
```

The package installs:

```text
/usr/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/coolercontrol-truenas-bridge
/etc/coolercontrol/plugins/coolercontrol-truenas-bridge/manifest.toml
/etc/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
```

It creates `config.json` only if it does not already exist.

## Install From Source

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev

git clone https://github.com/kamko/coolercontrol-truenas-bridge.git
cd coolercontrol-truenas-bridge

sudo ./scripts/install-plugin.sh
sudoedit /etc/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
sudo systemctl restart coolercontrold
```

CoolerControl should then show a `TrueNAS` device with one temperature source per discovered disk.

The plugin manifest runs the service as privileged so the config/API key can stay root-readable only.

## Configuration

Config path:

```text
/etc/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
```

Example:

```json
{
  "truenas": {
    "host": "truenas.local",
    "username": "coolercontrol",
    "api_key": "",
    "api_key_file": "/etc/coolercontrol/plugins/coolercontrol-truenas-bridge/api.key",
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

`disk_names` can stay empty to expose all disks returned by TrueNAS. Set it to a list like `["sda", "sdb"]` to limit the API call.

TrueNAS updates disk temperatures roughly every 5 minutes, so `poll_interval_seconds = 300` is the normal default.

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
