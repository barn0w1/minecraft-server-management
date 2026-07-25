# ADR-0001: Use modular monoliths

Status: Accepted

## Context

small-community向けself-hosted systemでは、独立deployment、scale、distributed transaction、network API versioningのcostに対する利益がない。一方、責務とdependency directionの分離は必要である。

## Decision

Control PlaneとNode Agentを、それぞれ一つのprocessと明確な内部moduleを持つmodular monolithとして構築する。domainごとのmicroserviceへ分割しない。

## Consequences

module ownershipとpublic surfaceを厳密に保つ必要がある。将来、実際の独立scale/failure isolation要件が生じた場合はprocess分割を再評価できる。

## Related documents

- [Design principles](../design-principles.md)
- [Architecture overview](../architecture/overview.md)
