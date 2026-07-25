# Architecture Reset plan

Status: Completed

## Goal

過剰にgeneralizedでfailure-centricだった初期設計を、Minecraft Serverを安全にスマートに自動運用するための一貫したv1 architectureへ置き換えます。

## Decisions completed

### Product and resource model

- `MinecraftServer`をprimary aggregateとした
- public resourceをMinecraftServer、Node、Snapshot、Operationへ整理した
- generic Workload domainを削除した
- itzg/minecraft-serverを唯一のruntime implementationとした
- 一つのNodeに同時に一つのMinecraftServerをallocateするv1 invariantを採用した

### Persistence and recovery unit

- `/data`だけでなくruntime configurationとserver-local secretを含む`Server Home`を定義した
- Server Home全体をrestic backup・restore対象とした
- Snapshotへspec generationとmanifestを関連付けた

### Agent communication

- JSON-RPC 2.0を共通RPC envelopeとして維持した
- raw QUICとcustom framingを廃止した
- HTTPS / HTTP/2とAgent-initiated `agent.sync`を採用した
- private PKIをv1要件から外し、server TLSとper-Node Agent Credentialへ簡素化した

### Distributed execution

- durable Operationとexplicit Stageを採用した
- Agent Commandをat-least-once deliveryとした
- Agent local journalでidempotent effectを保証する方針にした
- allocationごとのFencing Tokenを採用した
- normal timeout、disconnect、429、5xxをautomatic retryへ移した
- Incidentをunsafe contradictionに限定した

### Backup contract

- restic exit code 0とSnapshot ID取得をbackup成功とした
- backup成功後の追加repository verificationを必須にしない
- R2のdocumented consistencyとdurabilityを信頼する
- 一つのDeployment Restic Passwordを全repositoryで共有する
- passwordをControl Plane databaseではなくdeployment secret fileへ置く

### Implementation strategy

- Node layerから順番にhorizontal buildする計画を廃止した
- 手動登録Node上のreal itzg runtimeから始めるvertical sliceへ変更した
- durable execution、backup、Akamai lifecycleを後続milestoneとして分離した

## Files affected

current design、ADR、plan、reference、testing strategyを同じchangeで更新し、obsoleteなWorkloadとServer Data contractを削除しました。

## Exit conditions

- first reading pathだけでnew architectureを理解できる
- current designにraw QUIC、private PKI、generic Workload、empty restic passwordが残っていない
- Agent APIがJSON-RPC over HTTP/2 pull modelとして定義されている
- Server Homeがdataとruntime configurationのrecovery unitとして定義されている
- failure modelがrecovery action中心になっている
- next implementation milestoneがLocal Node vertical sliceとして定義されている
