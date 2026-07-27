#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || {
  echo "usage: $0 /etc/letsencrypt/live/LINEAGE" >&2
  exit 2
}

lineage=$(readlink -f -- "$1")
[[ -r ${lineage}/fullchain.pem && -r ${lineage}/privkey.pem ]] || {
  echo "Certbot lineage is incomplete: ${lineage}" >&2
  exit 1
}

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
install -d -m0755 /etc/letsencrypt/renewal-hooks/deploy
install -m0755 \
  "${repository_root}/deploy/certbot-deploy-hook.sh" \
  /etc/letsencrypt/renewal-hooks/deploy/50-mcserver-control-plane
temporary=$(mktemp /etc/mcserver/.certbot-lineage.XXXXXX)
trap 'rm -f -- "${temporary}"' EXIT
printf '%s\n' "${lineage}" >"${temporary}"
chmod 0644 "${temporary}"
chown root:root "${temporary}"
mv -f -- "${temporary}" /etc/mcserver/certbot-lineage
trap - EXIT
