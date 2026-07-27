#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 OUTPUT_DIRECTORY [COMMON_NAME]" >&2
  exit 2
}

[[ $# -ge 1 && $# -le 2 ]] || usage
command -v openssl >/dev/null 2>&1 || {
  echo "openssl is required" >&2
  exit 1
}

output_directory=$1
common_name=${2:-mcserver-agent-client-ca}
[[ -n ${common_name} ]] || {
  echo "COMMON_NAME must be non-empty" >&2
  exit 1
}

umask 077
install -d -m0700 -- "${output_directory}"
private_key="${output_directory}/agent-client-ca-private-key.pem"
certificate="${output_directory}/agent-client-ca.pem"

[[ ! -e ${private_key} && ! -e ${certificate} ]] || {
  echo "refusing to overwrite an existing agent client CA" >&2
  exit 1
}

openssl genpkey \
  -algorithm EC \
  -pkeyopt ec_paramgen_curve:P-256 \
  -out "${private_key}"
openssl req \
  -new \
  -x509 \
  -sha256 \
  -days 3650 \
  -key "${private_key}" \
  -subj "/CN=${common_name}" \
  -addext 'basicConstraints=critical,CA:TRUE,pathlen:0' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign' \
  -addext 'subjectKeyIdentifier=hash' \
  -out "${certificate}"
chmod 0600 "${private_key}"
chmod 0644 "${certificate}"

openssl pkey -in "${private_key}" -check -noout
openssl x509 -in "${certificate}" -checkend 31536000 -noout
openssl verify -CAfile "${certificate}" "${certificate}"

printf 'created private key: %s\ncreated certificate: %s\n' \
  "${private_key}" "${certificate}"
