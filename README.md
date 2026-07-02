# TrueNAS CoolerControl Sensors

Expose disk temperatures from a TrueNAS VM to CoolerControl running on a Proxmox host.

The bridge polls the TrueNAS WebSocket JSON-RPC API, converts Celsius values to the sysfs millidegree format CoolerControl expects, and writes files such as:

```text
/var/lib/truenas-coolercontrol-sensors/hdd_max_temp
/var/lib/truenas-coolercontrol-sensors/disks/sda_temp
```

CoolerControl can then use those files as **Custom Sensors -> File** and drive normal fan profiles from them.

## Why This Exists

With HBA passthrough, TrueNAS sees disk temperatures, but Proxmox controls the fans. CoolerControl already handles fan curves and hardware control well, so this service only exports the missing sensor data.

## TrueNAS Permissions

Create a TrueNAS API key for a service account that can call `disk.temperatures`.

Required role:

```text
REPORTING_READ
```

TrueNAS 25.04 and newer use the WebSocket API. This bridge uses `auth.login_ex` with `API_KEY_PLAIN`. Legacy `auth.login_with_api_key` is available only as a compatibility fallback.

## Install On Proxmox

You can either build locally on Proxmox or download a binary from the GitHub Actions artifacts.

```bash
sudo apt update
sudo apt install -y golang-go

git clone <this-repo> /opt/truenas-coolercontrol-sensors
cd /opt/truenas-coolercontrol-sensors

sudo ./scripts/install-proxmox.sh
sudoedit /etc/truenas-coolercontrol-sensors.json

sudo systemctl enable --now truenas-coolercontrol-sensors.service
sudo systemctl status truenas-coolercontrol-sensors.service
```

Build manually:

```bash
make test
make build
```

Check the exported files:

```bash
sudo ls -l /var/lib/truenas-coolercontrol-sensors
sudo cat /var/lib/truenas-coolercontrol-sensors/hdd_max_temp
```

If the file contains `42000`, CoolerControl will read it as `42 C`.

## CoolerControl Setup

1. Open CoolerControl.
2. Go to **Custom Sensors**.
3. Add a **File** temperature sensor.
4. Use this path:

```text
/var/lib/truenas-coolercontrol-sensors/hdd_max_temp
```

5. Create a normal graph fan profile based on that sensor.
6. Apply the profile to your HDD/case fan group.

Per-disk sensors are written under:

```text
/var/lib/truenas-coolercontrol-sensors/disks/
```

For fan control, start with `hdd_max_temp`; it is the cleanest signal for a disk bay.

## Configuration

Copy and edit:

```bash
sudo cp config.example.json /etc/truenas-coolercontrol-sensors.json
sudoedit /etc/truenas-coolercontrol-sensors.json
```

Important defaults:

```json
{
  "polling": {
    "poll_interval_seconds": 300,
    "stale_after_seconds": 900,
    "failsafe_temperature_c": 55
  }
}
```

TrueNAS only refreshes disk temperatures every 5 minutes, so polling much faster usually does not help.

If polling fails for too long, the bridge writes the fail-safe temperature to `hdd_max_temp`. That makes CoolerControl ramp fans instead of being stuck at a stale low value.

## Manual Test

```bash
/usr/local/bin/truenas-coolercontrol-sensors \
  --config /etc/truenas-coolercontrol-sensors.json \
  --once
```

Use `--print-sample` to show parsed temperatures without writing files:

```bash
/usr/local/bin/truenas-coolercontrol-sensors \
  --config /etc/truenas-coolercontrol-sensors.json \
  --once \
  --print-sample
```
