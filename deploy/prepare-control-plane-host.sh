#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 0 ]] || {
  echo "usage: sudo $0" >&2
  exit 2
}
[[ ${EUID} -eq 0 ]] || {
  echo "this preparation script must run as root" >&2
  exit 1
}

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

install -Dm0644 -- \
  "${repository_root}/deploy/systemd/mcserver-control-plane.sysusers.conf" \
  /etc/sysusers.d/mcserver-control-plane.conf
systemd-sysusers /etc/sysusers.d/mcserver-control-plane.conf

install -d -m0750 -o root -g mcserver \
  /etc/mcserver \
  /etc/mcserver/pki \
  /etc/mcserver/servers
install -d -m0700 -o root -g root \
  /etc/mcserver/credentials \
  /var/lib/mcserver-deploy

if [[ ! -e /etc/mcserver/deployment.toml ]]; then
  install -m0600 -o root -g root \
    "${repository_root}/deploy/production-deploy.toml.example" \
    /etc/mcserver/deployment.toml
fi
if [[ ! -e /etc/mcserver/credentials/r2-runtime.env ]]; then
  install -m0600 -o root -g root \
    "${repository_root}/deploy/systemd/r2-runtime.env.example" \
    /etc/mcserver/credentials/r2-runtime.env
fi
for secret in akamai-api-token r2-api-token; do
  if [[ ! -e /etc/mcserver/credentials/${secret} ]]; then
    install -m0600 -o root -g root /dev/null \
      "/etc/mcserver/credentials/${secret}"
  fi
done
if [[ ! -e /etc/mcserver/authorized_keys ]]; then
  install -m0640 -o root -g mcserver /dev/null \
    /etc/mcserver/authorized_keys
fi

ca_key=/etc/mcserver/credentials/agent-client-ca-private-key.pem
ca_certificate=/etc/mcserver/pki/agent-client-ca.pem
if [[ ! -e ${ca_key} && ! -e ${ca_certificate} ]]; then
  work=$(mktemp -d)
  trap 'rm -rf -- "${work}"' EXIT
  "${repository_root}/deploy/generate-agent-client-ca.sh" "${work}/agent-client-ca"
  install -m0600 -o root -g root \
    "${work}/agent-client-ca/agent-client-ca-private-key.pem" "${ca_key}"
  install -m0644 -o root -g mcserver \
    "${work}/agent-client-ca/agent-client-ca.pem" "${ca_certificate}"
elif [[ ! -e ${ca_key} || ! -e ${ca_certificate} ]]; then
  echo "agent client CA is incomplete; restore both files or remove both and rerun" >&2
  exit 1
fi

cat <<'MESSAGE'
Control-plane host directories are ready.

Edit:
  /etc/mcserver/deployment.toml
  /etc/mcserver/credentials/akamai-api-token
  /etc/mcserver/credentials/r2-api-token
  /etc/mcserver/authorized_keys

Server definitions belong in:
  /etc/mcserver/servers/

Deployment reports belong in:
  /var/lib/mcserver-deploy/
MESSAGE
