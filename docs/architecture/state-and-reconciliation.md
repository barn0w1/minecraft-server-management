# State and reconciliation

## State categories

### Desired state

Operatorまたは上位domainが要求する状態です。例: `MinecraftServer.spec.desired_state = Running`。

### Durable application state

identity、spec、operation intent、Incident、最後に受理したreportなど、process restart後も必要なstateです。

### Observation

external systemから得たtimestamp付きの事実です。例: Compute Instance status、Node Agent report、systemd unit state、restic Snapshot、Minecraft readiness。

### Derived status

desired state、durable state、fresh observationからControllerが導出する状態です。外部truthそのものではありません。

## Controller model

Controllerは一回のreconciliationでboundedな処理だけを行います。

```text
load resource and related durable state
  → read fresh observation or schedule read
  → validate identity and ownership
  → derive current status
  → persist at most one safe next intent/action
  → return next evaluation time
```

process内event、timer、notificationはreconciliationを早めるhintであり、正しさの前提ではありません。

## Mutation intent

external mutation前に、少なくとも次をdurableにします。

- target logical identity
- external identityまたはdiscovery key
- requested action
- idempotency/correlation identity
- generation
- started timestamp
- expected observation

mutation responseが失われても、restart後にread-only observationから結果を追跡できるようにします。

## Freshness

Observationにはsourceと`observed_at`を持たせます。古いObservationだけで`Ready`を維持しません。freshness thresholdはdomain/interface documentのinitial defaultとして定義し、configuration可能にします。

## Restart recovery

- Control Plane restart後はmemory上のAgent sessionを無効とする
- pending Operationとretry deadlineはdatabaseから再構築する
- startup inventoryを完了するまでdestructive mutationを抑止できる設計にする
- Node Agent restart後は新session IDとinitial reportを要求する
- eventの再配送を必要としない

## Finalization

logical resourceを削除しただけでexternal resourceが消えたとみなしません。

例: Node release

```text
Node desired lifecycle = Absent
  → ownershipを再確認
  → Compute Instance delete intent
  → provider inventoryでAbsentを確認
  → credential/authorizationを無効化
  → final durable recordを確定
```

## Concurrency

- resource generationまたはversionでstale writeを拒否する
- 一つのresourceに複数のmutation intentを同時に走らせない
- separate QUIC stream間のarrival orderをdomain orderingに使用しない
- JSON-RPC request IDをidempotency keyとみなさない
