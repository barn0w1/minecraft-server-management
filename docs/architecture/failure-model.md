# Failure model

Failure modelの目的は、すべてを精密に分類することではなく、**次に何をするかを一貫して決めること**です。

## Failure categories

| Category | Meaning | Default action |
| --- | --- | --- |
| `Invalid` | requestまたはSpecが実行不能 | retryせずOperationをFailedにする |
| `Transient` | network、429、5xx、temporary unavailable | backoff付きで自動retryする |
| `Conflict` | stale generation、既存stateとの競合 | stateを再読込してreconcileする |
| `UnknownOutcome` | mutation送信後にresultを受け取れない | observationまたはidempotent replayで収束する |
| `Unsafe` | identityやownershipが矛盾し安全な選択がない | Incidentを作り該当mutationを停止する |
| `Internal` | invariant violationまたはprogram bug | diagnosticを残しsubsystemを停止する |

## Common mapping

| Situation | Category |
| --- | --- |
| validation error | `Invalid` |
| request送信前のDNS/connect/TLS failure | `Transient` |
| read-only timeout | `Transient` |
| HTTP 429 | `Transient`; `Retry-After`を優先 |
| HTTP 502/503/504 | 通常`Transient` |
| provider create response loss | `UnknownOutcome`; ownership metadataでinventory |
| Agent Sync response loss | `UnknownOutcome`; same Commandを再配送可能 |
| restic exit code 0 | success |
| restic exit code 3 | `Invalid`またはsource read failureとしてFailed |
| Agent disconnected | `Transient`; reconnect待ち |
| stale generationまたはfencing token | `Conflict` |
| multiple active Node allocation | `Unsafe` |
| ownership metadataがstored identityと矛盾 | `Unsafe` |

## Retry policy

Transient failureはexponential backoffとjitterを使用します。

initial schedule:

```text
1s, 2s, 4s, 8s, 16s, 30s, 60s, 60s ...
```

- providerの`Retry-After`があれば優先する
- retry countだけでなくOperation deadlineを持つ
- next retry timeをOperationへ保存する
- Operatorからretry中であることを確認できる
- deadline超過後はOperationをFailedにするか、明示的にwaiting policyへ移す

## Unknown outcome recovery

Unknown outcomeは即Incidentではありません。operation kindごとのstandard recoveryを使用します。

### Agent Command

同じ`operation_id + stage`を再配送します。Agent journalが既存resultを返します。

### Compute Instance create

Operation ID、Deployment ID、Node IDに対応するprovider metadataでinventoryし、exactly oneならadoptします。zeroならvisibility delay後に同じlogical operationを再試行できます。multipleかつ一意に選べない場合だけUnsafeです。

### Compute Instance delete

stored Compute Instance IDをreadします。NotFoundならsuccess、存在すればdeleteを再試行します。ownership contradictionだけUnsafeです。

## Incident policy

Incidentは通常のretry queueではありません。次のような例外に限定します。

- 一つのMinecraftServerに複数のactive Nodeが存在する
- stale Nodeとcurrent Nodeの両方が同じServer Homeへ書き込める
- provider resource ownershipがstored identityと矛盾する
- restore先に未知のServer Homeがありoverwrite判断ができない
- database invariantが壊れてresource identityを確定できない

`Acknowledged`は認識済みを表すだけで、mutation gateを解除しません。解決時はOperatorが選択したremediationをEventへ記録します。

## Error reporting

Operation errorは少なくとも次を持ちます。

```text
category
code
message
source
attempt
retry_at
operator_action_required
```

external toolのraw stderr全体をpublic API contractにせず、redacted diagnosticとして保持します。
