#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
config_path="/etc/truenas-coolercontrol-sensors.json"
service_path="/etc/systemd/system/truenas-coolercontrol-sensors.service"
binary_path="/usr/local/bin/truenas-coolercontrol-sensors"

if ! command -v go >/dev/null 2>&1; then
  echo "Go is required to build the binary."
  echo "Install it first, for example: sudo apt install -y golang-go"
  exit 1
fi

cd "${repo_dir}"
go build -trimpath -ldflags="-s -w" -o "${binary_path}" ./cmd/truenas-coolercontrol-sensors

install -d -m 0755 /var/lib/truenas-coolercontrol-sensors

if [[ ! -f "${config_path}" ]]; then
  install -m 0600 "${repo_dir}/config.example.json" "${config_path}"
  echo "Created ${config_path}. Edit it before starting the service."
else
  echo "${config_path} already exists; leaving it unchanged."
fi

install -m 0644 "${repo_dir}/systemd/truenas-coolercontrol-sensors.service" "${service_path}"
systemctl daemon-reload

echo "Installed ${binary_path}."
echo "Installed ${service_path}."
echo "Next:"
echo "  sudoedit ${config_path}"
echo "  sudo systemctl enable --now truenas-coolercontrol-sensors.service"
