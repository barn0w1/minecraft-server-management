# ADR-0005: Use QUIC, mTLS, and JSON-RPC for the Agent Protocol

Status: Accepted

## Context

一つのoutbound long-lived connection上で双方initiated RPCを多重化でき、managed Nodeへinbound management portを増やさずに済む。JSON-RPCはsimpleでtyped schemaをproject側で定義できる。

## Decision

Control PlaneとNode Agentのremote protocolにraw QUIC v1/TLS 1.3を使用し、application envelopeに限定したJSON-RPC 2.0 profileを使用する。一request/responseは一bidirectional stream、notificationは一unidirectional streamへmappingする。

## Consequences

HTTP ecosystemを直接利用しないためframing、limit、deadline、version、error、idempotencyを明示する必要がある。0-RTT application dataはreplay riskのため使用しない。

## Related documents

- [Design principles](../design-principles.md)
- [Architecture overview](../architecture/overview.md)
