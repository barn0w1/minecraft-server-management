# Foundation plan

Status: Proposed

## Goal

current documentationを実装可能な最小contractへし、Control Plane、Node Agent、CLIを安全に開発できるRust foundationを作る準備を完了します。

このplanの現在phaseではcodeを追加しません。

## Documentation gate

実装開始前に次がreview済みであることを要求します。

- Vision、Scope、Terminology
- modular monolithとmodule dependency
- state/reconciliation model
- failure/Incident model
- Operator API boundary
- Agent Protocol layering
- private PKIとenrollment flow
- Node Management v1 scopeとacceptance
- P0 open questionが明示されている

## Planned implementation slices

### Slice 1: workspace skeleton

将来追加予定:

- `control-plane`
- `node-agent`
- `cli`
- `protocol`

空のgeneric crateを増やしません。

### Slice 2: common runtime foundation

- typed configuration
- structured logging and redaction
- shutdown/task supervision
- clock/randomness boundary where needed
- error reporting

### Slice 3: Control Plane persistence

- SQLite connection and migration
- application-owned transaction boundary
- operation/Incident primitives
- restart test foundation

### Slice 4: Operator API vertical slice

- Unix socket lifecycle
- JSON-RPC request/response
- peer identity/audit context
- one read-only health/version method
- `mcserverctl` typed client

### Slice 5: Agent protocol skeleton

- QUIC endpoint/client
- server-auth TLS test profile
- framing/parser limits
- request/response stream
- notification stream
- protocol harness

## Out of scope

- Akamai mutation
- real Agent enrollment issuer
- Node resource lifecycle
- Podman/restic/Minecraft integration
- Discord Bot

## Exit conditions

- repository structureがdocumented boundaryと一致する
- processがclean startup/shutdownできる
- Operator APIのlocal vertical sliceがtestされる
- Agent protocol framing/session skeletonがtestされる
- SQLite restart testが動く
- secretをlogしないfoundationがある
