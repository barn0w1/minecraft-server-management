# minecraft-server-management

Akamai Cloud の一時 VM で Minecraft サーバーを実行し、ワールドデータを
Cloudflare R2 に永続化する Rust 製コントロールプレーンです。

Minecraft の実行中だけ VM を保持します。停止要求を受けると Minecraft を正常停止し、
`/data` 全体を restic snapshot として R2 に保存してから VM を削除します。次回は最新
snapshot を新しい VM へ復元します。

## v0.2.0 の運用モデル

- `Server` ごとに Akamai の region、instance type、image、firewall、Minecraft 設定を管理
- server 定義は TOML ファイルで宣言し、`mcserverctl server apply` で作成・更新
- R2 の保存先は `servers/<Server UUID>/restic` として自動割当
- restic repository は初回起動時に自動作成
- restic は `--insecure-no-password` を常に使用し、パスワードを保管しない
- 一時 VM へ渡す R2 credential は、その server の prefix のみに制限された短期 credential
- global 設定は利用可能な Akamai resource の allowlist と同時実行上限だけを保持
- node agent は mTLS で control plane へ接続し、VM 側に長期 cloud credential を保存しない
- Certbot の更新 hook が証明書を検証して反映し、異常時は直前の証明書へ戻す

## 最初に読む文書

1. [外部インフラの前提](docs/production-prerequisites.ja.md)
2. [クリーンなホストへの本番導入](docs/production-installation.ja.md)
3. [Server の作成と通常運用](docs/operations.ja.md)

設計と開発については
[アーキテクチャ](docs/architecture.md)、
[client API](docs/client-api.md)、
[開発と検証](docs/development.md)
を参照してください。

## 構成

```text
crates/
  mcserver-control-plane/  SQLite、reconciler、Akamai/R2、client/agent API
  mcserver-node-agent/     Podman、Minecraft、restic restore/snapshot
  mcserver-protocol/       client/agent 間の JSON-RPC DTO
deploy/                    本番インストーラー、systemd、Certbot hook
migrations/                SQLite migration
scripts/                   ローカルおよび provider E2E
```

固定 toolchain は Rust 1.97.1、edition は 2024 です。

## 開発時の検証

全検証は1 command で実行できます。

```bash
scripts/verify.sh
```

内部の deterministic E2E は fake Podman、fake restic、fake provider API を利用し、課金
resource を作成しません。実際の Akamai VM を使う acceptance は
`deploy/production_deploy.py deploy --go-live` だけが実行します。

## 重要な境界

- Minecraft EULA への同意は server 定義で明示する必要があります。
- `/data` 内のファイルを control plane は解釈・編集しません。
- 稼働中の server 定義は変更できません。停止完了後に `server apply` します。
- storage backend と repository は Server 作成後に変更できません。
- SQLite は desired state と監査履歴の正本、停止済み snapshot は R2 がデータの正本です。
- Discord bot や Web UI は Unix socket の client API を利用する別 client として実装します。
