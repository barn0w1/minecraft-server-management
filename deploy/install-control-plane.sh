#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: sudo $0 PATH_TO_MCSERVER_CONTROL_PLANE_BINARY [PATH_TO_MCSERVERCTL_BINARY]" >&2
  exit 2
}

[[ $# -ge 1 && $# -le 2 ]] || usage
[[ ${EUID} -eq 0 ]] || { echo "this installer must run as root" >&2; exit 1; }

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
[[ -x $1 ]] || { echo "control-plane binary is not executable: $1" >&2; exit 1; }
binary=$(realpath -- "$1")
cli_candidate=${2:-"$(dirname -- "${binary}")/mcserverctl"}
[[ -x ${cli_candidate} ]] || { echo "mcserverctl binary is not executable: ${cli_candidate}" >&2; exit 1; }
cli=$(realpath -- "${cli_candidate}")

"${binary}" --version
"${cli}" --version
install -Dm0755 -- "${binary}" /usr/local/bin/mcserver-control-plane
install -Dm0755 -- "${cli}" /usr/local/bin/mcserverctl
install -Dm0755 -- \
  "${repository_root}/deploy/production_deploy.py" \
  /usr/local/libexec/mcserver/deploy/production_deploy.py
install -Dm0755 -- \
  "${repository_root}/deploy/prepare-control-plane-host.sh" \
  /usr/local/libexec/mcserver/deploy/prepare-control-plane-host.sh
install -Dm0755 -- \
  "${repository_root}/deploy/generate-agent-client-ca.sh" \
  /usr/local/libexec/mcserver/deploy/generate-agent-client-ca.sh
install -Dm0644 -- \
  "${repository_root}/deploy/production-deploy.toml.example" \
  /usr/local/libexec/mcserver/deploy/production-deploy.toml.example
install -Dm0644 -- \
  "${repository_root}/deploy/systemd/mcserver-control-plane.sysusers.conf" \
  /usr/local/libexec/mcserver/deploy/systemd/mcserver-control-plane.sysusers.conf
install -Dm0644 -- \
  "${repository_root}/deploy/systemd/r2-runtime.env.example" \
  /usr/local/libexec/mcserver/deploy/systemd/r2-runtime.env.example
install -Dm0755 -- \
  "${repository_root}/scripts/live_akamai_e2e.py" \
  /usr/local/libexec/mcserver/scripts/live_akamai_e2e.py
install -Dm0644 -- \
  "${repository_root}/scripts/local_e2e.py" \
  /usr/local/libexec/mcserver/scripts/local_e2e.py
install -Dm0644 -- \
  "${repository_root}/deploy/production-deploy.toml.example" \
  /usr/local/share/mcserver/production-deploy.toml.example
install -Dm0644 -- \
  "${repository_root}/examples/community-server.toml" \
  /usr/local/share/mcserver/community-server.toml
install -Dm0644 -- \
  "${repository_root}/deploy/systemd/mcserver-control-plane.service" \
  /etc/systemd/system/mcserver-control-plane.service
install -Dm0644 -- \
  "${repository_root}/deploy/systemd/mcserver-control-plane.sysusers.conf" \
  /etc/sysusers.d/mcserver-control-plane.conf
systemd-sysusers /etc/sysusers.d/mcserver-control-plane.conf
install -d -m0750 -o root -g mcserver \
  /etc/mcserver /etc/mcserver/pki /etc/mcserver/servers
install -d -m0700 -o root -g root /etc/mcserver/credentials
if [[ ! -e /etc/mcserver/control-plane.env ]]; then
  install -m0640 -o root -g mcserver \
    "${repository_root}/deploy/systemd/mcserver-control-plane.env.example" \
    /etc/mcserver/control-plane.env
fi
systemctl daemon-reload
cat <<'MESSAGE'
Installed but not enabled.
Populate /etc/mcserver/control-plane.env, /etc/mcserver/pki, /etc/mcserver/credentials,
and /etc/mcserver/authorized_keys. Copy r2-runtime.env.example to the credential
directory without adding long-lived R2 access keys. Keep MCSERVER_AKAMAI_LIVE_ENABLED=false for the first start.
Then validate and enable:
  systemctl start mcserver-control-plane.service
  systemctl status mcserver-control-plane.service
  mcserverctl --socket /run/mcserver/control-plane.sock ping
  systemctl enable mcserver-control-plane.service
See docs/production-installation.ja.md before enabling billable creation.

After the first deployment, update and verification commands are available at:
  /usr/local/libexec/mcserver/deploy/production_deploy.py
MESSAGE
