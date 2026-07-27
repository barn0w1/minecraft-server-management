#!/usr/bin/env bash
set -euo pipefail

lineage_file=/etc/mcserver/certbot-lineage
[[ -r ${lineage_file} ]] || exit 0
expected_lineage=$(<"${lineage_file}")
renewed_lineage=${RENEWED_LINEAGE:-}
[[ -n ${renewed_lineage} ]] || exit 0
[[ $(readlink -f -- "${renewed_lineage}") == "${expected_lineage}" ]] || exit 0

source_certificate=${renewed_lineage}/fullchain.pem
source_chain=${renewed_lineage}/chain.pem
source_private_key=${renewed_lineage}/privkey.pem
certificate=/etc/mcserver/pki/remote-tls-fullchain.pem
trust_anchor=/etc/mcserver/pki/remote-tls-root-ca.pem
private_key=/etc/mcserver/credentials/remote-tls-private-key.pem
certificate_stage=${certificate}.new
private_key_stage=${private_key}.new
certificate_backup=${certificate}.previous
private_key_backup=${private_key}.previous

cleanup() {
  rm -f -- "${certificate_stage}" "${private_key_stage}"
}
trap cleanup EXIT

install -m0644 -o root -g mcserver -- "${source_certificate}" "${certificate_stage}"
install -m0600 -o root -g root -- "${source_private_key}" "${private_key_stage}"

openssl x509 -in "${certificate_stage}" -noout >/dev/null
openssl pkey -in "${private_key_stage}" -check -noout >/dev/null
openssl verify \
  -purpose sslserver \
  -CAfile "${trust_anchor}" \
  -untrusted "${source_chain}" \
  "${source_certificate}" >/dev/null
certificate_key=$(
  openssl x509 -in "${certificate_stage}" -pubkey -noout |
    openssl pkey -pubin -outform DER 2>/dev/null |
    sha256sum |
    cut -d' ' -f1
)
private_key_public=$(
  openssl pkey -in "${private_key_stage}" -pubout -outform DER 2>/dev/null |
    sha256sum |
    cut -d' ' -f1
)
[[ ${certificate_key} == "${private_key_public}" ]]

cp -a -- "${certificate}" "${certificate_backup}"
cp -a -- "${private_key}" "${private_key_backup}"
mv -f -- "${certificate_stage}" "${certificate}"
mv -f -- "${private_key_stage}" "${private_key}"

healthy=false
systemctl restart mcserver-control-plane.service || true
for _ in $(seq 1 30); do
  if /usr/local/bin/mcserverctl \
    --socket /run/mcserver/control-plane.sock \
    ping >/dev/null 2>&1; then
    healthy=true
    break
  fi
  sleep 1
done
if [[ ${healthy} == true ]]; then
  rm -f -- "${certificate_backup}" "${private_key_backup}"
  exit 0
fi

mv -f -- "${certificate_backup}" "${certificate}"
mv -f -- "${private_key_backup}" "${private_key}"
systemctl restart mcserver-control-plane.service
exit 1
