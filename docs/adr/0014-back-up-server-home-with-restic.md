# ADR-0014: Back up Server Home with restic and trust successful completion

Status: Accepted

Supersedes: ADR-0010

## Context

world dataだけでなく、そのdataを起動するitzg configurationも同じrecovery unitとして保存する必要があります。また、resticやR2の内部整合性をsystemが毎回再検証することは、実装と運用を複雑にします。

repositoryごとのrandom passwordはControl Plane databaseやsecret state喪失時のrecoveryを難しくします。

## Decision

- backup単位を`Server Home`全体とする
- Server Homeは`data/`、`manifest.json`、server-local secretを含む
- 一つのMinecraftServerにつき一つのrestic repositoryを使用する
- backendはCloudflare R2とする
- すべてのrepositoryで一つのDeployment Restic Passwordを共有する
- passwordはControl Plane databaseではなくoperator-managed deployment secret fileへ保存する
- `restic backup` exit code 0とstructured outputからのSnapshot ID取得をbackup成功とする
- backup成功条件へ追加`restic snapshots`、`restic check`、full-read verificationを含めない
- R2のdocumented consistencyとdurabilityを信頼する

## Consequences

Snapshot単体からdataとruntime configurationを復元できます。password運用は単純になりますが、Deployment Restic Passwordを失うと全repositoryをrestoreできないため、Operatorはdeployment recovery materialとして別途保管する必要があります。

restic exit code 3はincomplete Snapshotであるため成功扱いしません。

## Related documents

- [Server Home domain](../domains/server-home.md)
- [Security model](../architecture/security-model.md)
