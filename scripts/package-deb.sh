#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_dir}"

package_name="coolercontrol-truenas-bridge"
version="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/version = "([^"]+)"/\1/')"
architecture="${1:-$(dpkg --print-architecture)}"
dist_dir="${repo_dir}/dist"
work_dir="${dist_dir}/deb-root"
plugin_id="coolercontrol-truenas-bridge"
plugin_dir="/var/lib/coolercontrol/plugins/${plugin_id}"
share_dir="/usr/share/${package_name}"

binary_path="${repo_dir}/target/release/${package_name}"
if [[ ! -x "${binary_path}" ]]; then
  echo "Missing release binary: ${binary_path}" >&2
  echo "Run cargo build --release first." >&2
  exit 1
fi

rm -rf "${work_dir}"
install -d \
  "${work_dir}/DEBIAN" \
  "${work_dir}${plugin_dir}" \
  "${work_dir}${share_dir}"

install -m 0755 "${binary_path}" "${work_dir}${plugin_dir}/${package_name}"
install -m 0644 plugin-files/config-example.json "${work_dir}${share_dir}/config-example.json"
install -m 0644 plugin-files/manifest.toml "${work_dir}${plugin_dir}/manifest.toml"

installed_size="$(du -ks "${work_dir}" | cut -f1)"
cat > "${work_dir}/DEBIAN/control" <<CONTROL
Package: ${package_name}
Version: ${version}
Section: utils
Priority: optional
Architecture: ${architecture}
Maintainer: kamko
Installed-Size: ${installed_size}
Depends: coolercontrold | coolercontrol
Description: CoolerControl plugin exposing TrueNAS disk temperatures
 Exposes TrueNAS disk temperatures as CoolerControl temperature sources
 for HBA passthrough setups where TrueNAS sees disks and Proxmox controls fans.
CONTROL

cat > "${work_dir}/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e

plugin_dir="/var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge"
config_path="${plugin_dir}/config.json"
example_path="/usr/share/coolercontrol-truenas-bridge/config-example.json"

if [ ! -f "${config_path}" ]; then
  install -d -m 0755 "${plugin_dir}"
  install -m 0600 "${example_path}" "${config_path}"
fi

cat <<'MESSAGE'

coolercontrol-truenas-bridge installed.

Next:
  sudoedit /var/lib/coolercontrol/plugins/coolercontrol-truenas-bridge/config.json
  sudo systemctl restart coolercontrold

MESSAGE

exit 0
POSTINST
chmod 0755 "${work_dir}/DEBIAN/postinst"

package_path="${dist_dir}/${package_name}_${version}_${architecture}.deb"
dpkg-deb --build --root-owner-group "${work_dir}" "${package_path}"
echo "${package_path}"
