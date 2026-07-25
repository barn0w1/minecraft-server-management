# State and reconciliation

## State categories

### Spec

Operatorまたはautomation policyが要求するdesired configurationです。Spec変更ごとに`generation`が増えます。

### Durable execution state

Operation、stage、attempt、deadline、next retry、allocation、Fencing Tokenなど、process restart後も必要なstateです。

### Observation

Akamai、Agent、systemd、Podman、RCON、resticから得たtimestamp付き事実です。

### Status

Spec、Operation、fresh Observationから導出するcurrent summaryです。Statusは外部systemの代替truthではありません。

## Operation model

Operationは次のphaseを持ちます。

```text
Pending → Running → Waiting → Running → Succeeded
                    │                    └→ Failed
                    └────────────────────→ Canceled
```

- `Pending`: 実行可能になるのを待つ
- `Running`: provider callまたはAgent Stageを実行中
- `Waiting`: dependency、retry time、observation、Agent reconnectを待つ
- terminal phase: `Succeeded`、`Failed`、`Canceled`

Retryは新しいOperationを増やさず、同じOperationの`attempt`と`next_attempt_at`を更新します。

## Operation Stage

Operationはexplicitなstage machineを持ちます。例:

```text
server.stop
  → request_graceful_stop
  → wait_runtime_stopped
  → backup_server_home
  → release_node
```

Agent Commandのidempotency keyは`operation_id + stage`です。payload hashが一致する同じCommandは、Agent journal内の既存stateまたはresultを返します。payloadが違う場合はprotocol conflictです。

## Controller model

一回のreconciliationはboundedです。

```text
1. resource、active Operation、Observationを読む
2. generation、allocation、Fencing Tokenを検証する
3. current Conditionを導出する
4. safe next actionを最大一つ決める
5. transactionでstageまたはretry timeを保存する
6. external callを行う場合はresultを保存して終了する
```

長時間sleep、busy wait、一つのhandler内でのend-to-end workflow完走を行いません。

## Agent delivery semantics

- Agent Sync responseを失ってもCommandが実行された可能性がある
- Control Planeは同じCommandを再配送できる
- Agentはlocal journalからRunningまたはterminal resultを返す
- JSON-RPC request IDはcorrelationだけに使う
- Operation IDとStageがeffect identityになる
- Command arrival orderをbusiness orderingに使わない

## Fencing

MinecraftServerのactive allocationごとにmonotonic `fencing_token`を発行します。Agentは受理済みtokenより古いCommandを拒否します。

```text
old Node token 14
new Node token 15

old Nodeが復帰してtoken 14のCommandを受信
  → stale_allocationとして拒否
```

## Freshness

Observationは`observed_at`とsourceを持ちます。Agent liveness、runtime readiness、player countなどはdomainごとのfreshness thresholdを超えたらUnknownへ戻します。

古いObservationだけで`Ready=True`を維持しません。

## Restart recovery

### Control Plane restart

- active OperationをSQLiteからloadする
- expired retryをscheduleする
- Agent SessionをUnknownへ戻す
- provider mutationをblind repeatせず、operation kindに応じてreadまたはretryする

### Agent restart

- 新しいSession IDを作る
- local journalをloadする
- local runtimeを観測する
- unfinished commandとterminal resultを次のAgent Syncで報告する

## Concurrency

- MinecraftServerごとにactive mutating Operationは最大一つ
- Nodeごとにactive Allocationは最大一つ
- AgentはMinecraftServerごとにmutating Stageをserializeする
- Spec generationが変わった場合、古いOperationはcancelまたは新Specに不要なら収束終了する
- optimistic concurrencyでstale database writeを拒否する

## Conditions

initial Condition set:

- `Ready`
- `Progressing`
- `Degraded`
- `NodeAvailable`
- `RuntimeReady`
- `BackupAvailable`

Conditionは`status`、`reason`、`message`、`observed_generation`、`last_transition_time`を持ちます。
