# 開発と検証

## Toolchain

- Rust 1.97.1 (`rust-toolchain.toml`)
- edition 2024
- Python 3
- OpenSSL
- systemd tools (`systemd-analyze`、`systemd-sysusers`)

```bash
rustup toolchain install 1.97.1 \
  --profile minimal \
  --component rustfmt,clippy
```

## 必須検証

まとめて実行:

```bash
scripts/verify.sh
```

個別の内容:

```bash
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace

python3 -m py_compile scripts/*.py scripts/fakes/*.py deploy/*.py
python3 -m unittest discover -s deploy -p 'test_*.py'
bash -n deploy/*.sh

python3 scripts/deterministic_e2e.py
python3 scripts/remote_provider_e2e.py
```

CI はこの順序を実行します。最後の2本は real Rust daemon と fake external dependency の
process-level E2E で、課金 resource や real credential を使いません。

## E2E の層

| Test | 実物 | fake | 目的 |
|---|---|---|---|
| unit | domain/parser/repository | 外部全部 | invariants |
| deterministic | control plane、agent、SQLite | Podman、restic | lifecycle、retry、cleanup |
| remote provider | 上記 + TLS + HTTP adapter | Akamai/R2 API、Podman、restic | mTLS、scoped credential、uncertain response |
| local E2E | Podman、restic、Minecraft | なし | host integration |
| live acceptance | Akamai、R2、Minecraft | なし | production 2世代 |

## 境界

- domain は wire DTO、SQLx、Tokio、Podman、restic に依存しない
- `mcserver-protocol` は wire type だけを持つ
- transport handler は application service へ委譲する
- `/data` は不透明
- DB が正本で queue は best effort
- external mutation は idempotent または uncertain response から回復可能にする
- provider delete は ID だけで行わず、label と scope tag を再検証する
- snapshot 公開前に fencing token を検証する
- migration file は release 後に書き換えず、新しい番号を追加する

## 命名

- package: `kebab-case`
- Rust module/function/variable、SQL/JSON field: `snake_case`
- type/trait/variant: `UpperCamelCase`
- constant: `SCREAMING_SNAKE_CASE`
- JSON-RPC method: `resource.verb`
- persistent timestamp: Unix epoch milliseconds、suffix `_at_ms`

## External process

- shell string ではなく argument API を使う
- timeout と bounded diagnostic を持たせる
- restic は全 command で `--insecure-no-password` と `--retry-lock` を使う
- remote root agent は host access、local rootless agent は `podman unshare` を使う
- node systemd hardening は rootful Podman の `/var`、`/run`、sysctl、setuid helper を妨げない

## Git

commit author/committer:

```text
barn0w1 <yuito.kiuchi.dev@gmail.com>
```

Conventional Commit の imperative subject を基本とします。
