#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "${repository_root}"

cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace

python3 -m py_compile scripts/*.py scripts/fakes/*.py deploy/*.py
python3 -m unittest discover -s deploy -p 'test_*.py'
bash -n deploy/*.sh scripts/*.sh

test_directory=$(mktemp -d)
trap 'rm -rf -- "${test_directory}"' EXIT
sed 's#/usr/local/bin/mcserver-control-plane#/usr/bin/true#g' \
  deploy/systemd/mcserver-control-plane.service \
  >"${test_directory}/mcserver-control-plane.service"
systemd-analyze verify "${test_directory}/mcserver-control-plane.service"
systemd-sysusers --dry-run \
  "${repository_root}/deploy/systemd/mcserver-control-plane.sysusers.conf"
python3 scripts/generate_spdx_sbom.py \
  --output "${test_directory}/SBOM.spdx.json" \
  --name minecraft-server-management-verification \
  --namespace https://github.com/barn0w1/minecraft-server-management/spdx/verification
python3 -m json.tool "${test_directory}/SBOM.spdx.json" >/dev/null

cargo build --locked --workspace
python3 scripts/deterministic_e2e.py
python3 scripts/remote_provider_e2e.py

printf '%s\n' 'all repository verification passed'
