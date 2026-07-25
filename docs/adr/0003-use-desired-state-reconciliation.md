# ADR-0003: Use desired-state reconciliation

Status: Accepted

## Context

cloud API、Node reboot、process restart、network interruptionは通常事象であり、一回限りのimperative scriptではdurable lifecycleを説明できない。

## Decision

resourceをSpec、Observation、Statusとして管理し、level-triggered Controllerがdesired stateへ継続的にreconcileする。

## Consequences

event deliveryへ依存せずrestart recoveryできる。代わりにstate model、freshness、idempotency、operation journalが必要になる。

## Related documents

- [Design principles](../design-principles.md)
- [Architecture overview](../architecture/overview.md)
