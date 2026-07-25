# Failure model

すべてのdomainとexternal adapterで共通するfailure languageを使用します。

## 1. Expected domain outcome

正常なcontrol flowとして扱える結果です。

- validation rejection
- requested resourceのnormal absence
- immutable field conflict
- operationが既に完了している
- desired lifecycleにより処理不要

Incidentを作らず、typed resultとして返します。

## 2. Retryable external failure

read-only operationやconnectionの一時的failureです。

- DNS/connect/TLS failure before a mutation is sent
- read-only timeout
- rate limiting
- temporary server error on an observation request
- transient Agent disconnect

bounded exponential backoff、jitter、providerのretry hint、freshness expirationを使用します。無期限に不可視なretryをしません。

## 3. Blocking external uncertainty or contradiction

自動的に安全な次のmutationを選べない状態です。

- mutation送信後にresponseを失い、結果を一意に観測できない
- ownership tagまたはexternal identityの矛盾
- duplicate resource
- certificate identityとdatabase identityの不一致
- restore/backupのterminal resultが不明

同じmutationをblind retryせずread-only observationへ移ります。boundedな確認後も解決しなければIncidentを作成し、affected scopeのmutationを停止します。

## 4. Internal invariant violation

program bug、database corruption、process内の不可能なstateなど、安全な継続が保証できない状態です。可能な範囲でdiagnosticを永続化し、processまたはsubsystemをfail closedにします。`panic!`はこのclassに限定します。

## Mutation response classification

| Situation | Classification |
| --- | --- |
| request送信前のDNS/connect/TLS failure | definitely not attempted; retryable after reevaluation |
| request body送信開始後のtimeout/disconnect | uncertain |
| complete expected 2xx response | accepted response; external stateは後続observationで確認 |
| malformed/truncated 2xx response | uncertain |
| documented complete 4xx rejection | definite rejection |
| 429 with retry guidance | retryable rejection |
| mutation requestへの500/502/503/504 | uncertain unless API contract proves non-execution |
| read-only timeout/5xx | retryable read failure |

## Incident lifecycle

```text
Open → Acknowledged → Resolved
```

- `Acknowledged`はoperatorが認識したことだけを示し、mutation gateを解除しない
- `Resolved`はoperatorの明示操作とaudit eventを必要とする
- resolve後も、以前の矛盾が消えたことを新しいread-only observationで確認してからmutationを再開する
- unaffected resourceとread-only observationは継続する
