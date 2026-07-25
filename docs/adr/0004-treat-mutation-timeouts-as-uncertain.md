# ADR-0004: Treat external mutation timeouts as uncertain

Status: Accepted

## Context

network responseの喪失はexternal operationの非実行を意味しない。blind retryはduplicate resource、data corruption、誤削除を起こし得る。

## Decision

request送信後のtimeout、disconnect、malformed success responseを、成功でも失敗でもなくuncertainとして扱う。同じmutationをblind retryせずread-only observationで確定する。

## Consequences

mutation前のdurable intent、ownership/discovery identity、bounded observation、Incident modelが必要になる。自動進行より安全停止を優先する。

## Related documents

- [Design principles](../design-principles.md)
- [Architecture overview](../architecture/overview.md)
