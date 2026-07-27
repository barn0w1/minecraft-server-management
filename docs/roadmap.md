# 現在の状態と今後

## v0.2.0

本番の基本 lifecycle は完成しています。

- declarative TOML による Server の create/apply
- Server ごとの Akamai compute と Minecraft process 設定
- global Akamai allowlist と active instance 上限
- Server ごとの R2 prefix と restic repository 自動初期化
- passwordless restic
- prefix-scoped R2 temporary credentials
- one-time enrollment と mTLS node agent
- uncertain create/delete の収束
- stop、snapshot 公開、VM 削除、次世代 restore
- Certbot deploy hook
- deterministic local/remote E2E と billable two-generation acceptance

## 次に追加する場合の優先順

1. Discord bot または Web UI を既存 client API の外部 client として実装
2. snapshot 一覧、明示 rollback、retention、prune、integrity check
3. metrics と失敗通知
4. 稼働中 restart を含む定期 production acceptance
5. 必要性が生じた場合だけ remote authenticated client gateway

SQLite backup、複数 control plane、汎用 provider framework は、個人運用の実際の要件が
生じるまで追加しません。
