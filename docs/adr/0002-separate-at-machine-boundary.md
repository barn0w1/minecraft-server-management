# ADR-0002: Separate Control Plane and Node Agent at the machine boundary

Status: Accepted

## Context

Control Planeはcentral durable stateとpolicyを所有するが、Node上のsystemd、filesystem、Podman、Minecraft local protocolへ直接アクセスできない。

## Decision

中央の`mcserver-control-plane`と各managed Node上の`mcserver-node-agent`を別processとする。分離理由はmachine/locality/security boundaryであり、microservice architectureではない。

## Consequences

cross-machine contract、identity、reconnect、partial failureを設計する必要がある。双方のprocess内部はmodular monolithとして維持する。

## Related documents

- [Design principles](../design-principles.md)
- [Architecture overview](../architecture/overview.md)
