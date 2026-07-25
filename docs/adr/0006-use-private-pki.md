# ADR-0006: Use a private PKI with an offline Root CA

Status: Accepted

## Context

Node Agentはpublic internet上のstable endpointへ接続するが、public CAだけではNode client identityを発行・lifecycle管理できない。

## Decision

Control Plane server identityとNode Agent client identityにprivate PKIを使用する。Root CA private keyはofflineに保管し、server issuerとAgent issuerを分離する。

## Consequences

CA custody、enrollment、rotation、authorization、issuer compromise responseが必要になる。初期revocationはshort-lived certificateとserver-side active Node authorizationを組み合わせる。

## Related documents

- [Design principles](../design-principles.md)
- [Architecture overview](../architecture/overview.md)
