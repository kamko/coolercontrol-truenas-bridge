#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
plugin_dir="/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge"
binary_path="${plugin_dir}/coolercontrol-truenas-bridge"
config_path="${plugin_dir}/config.json"

if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust/Cargo is required to build the plugin."
  echo "Install Rust first, then rerun this script."
  exit 1
fi

cd "${repo_dir}"
cargo build --release

install -d -m 0755 "${plugin_dir}"
install -m 0755 "target/release/coolercontrol-truenas-bridge" "${binary_path}"
install -m 0644 "plugin-files/manifest.toml" "${plugin_dir}/manifest.toml"

if [[ ! -f "${config_path}" ]]; then
  install -m 0600 "plugin-files/config-example.json" "${config_path}"
  echo "Created ${config_path}. Edit it before restarting CoolerControl."
else
  echo "${config_path} already exists; leaving it unchanged."
fi

echo "Installed CoolerControl plugin in ${plugin_dir}."
echo "Next:"
echo "  sudoedit ${config_path}"
echo "  sudo systemctl restart coolercontrold"
