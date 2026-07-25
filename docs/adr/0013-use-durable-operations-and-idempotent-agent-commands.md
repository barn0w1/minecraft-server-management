# ADR-0013: Use durable Operations and idempotent Agent Commands

Status: Accepted

Supersedes: ADR-0004

## Context

network response loss、Control Plane restart、Agent restartにより、Commandが実行されたかをrequest/responseだけで確定できない場合があります。

すべてのunknown outcomeをIncidentへ移すとautomationが止まり、exactly-once deliveryを実装しようとするとsystemが複雑になります。

## Decision

- long-running mutationをdurable `Operation`として追跡する
- Operationはexplicit Stage、attempt、deadline、next retryを持つ
- Agent Commandのeffect identityを`operation_id + stage`とする
- Control PlaneはCommandをat-least-onceで再配送できる
- Agentはlocal journalへpayload hash、state、resultを保存する
- same Commandは既存stateまたはresultを返す
- allocationごとのFencing Tokenでstale Nodeを拒否する
- timeoutやdisconnectはoperation-specific recoveryで自動収束する
- Incidentはunsafe contradictionだけに限定する

## Consequences

Control PlaneとAgent双方にdurable stateが必要ですが、message brokerやdistributed transactionは不要です。operation statusをOperatorへ説明でき、restart testでbehaviorを検証できます。

## Related documents

- [State and reconciliation](../architecture/state-and-reconciliation.md)
- [Failure model](../architecture/failure-model.md)
