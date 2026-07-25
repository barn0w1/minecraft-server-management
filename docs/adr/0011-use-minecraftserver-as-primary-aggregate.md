# ADR-0011: Use MinecraftServer as the primary aggregate and itzg as the sole runtime

Status: Accepted

## Context

以前の設計はNode、Workload、Server Data、Minecraft Serverを対等な主要domainとして扱い、general-purpose orchestratorに近いscopeを持っていました。

実際のproduct requirementはitzg/minecraft-serverを使ってMinecraft Java serverだけを管理することです。

## Decision

- `MinecraftServer`をprimary aggregateにする
- public resourceを`MinecraftServer`、`Node`、`Snapshot`、`Operation`へ絞る
- generic `Workload` resourceを削除する
- v1の唯一のruntime implementationをitzg/minecraft-serverとする
- Podman、Quadlet、systemdをinternal `Server Runtime`として使用する
- 一つのNodeに同時に一つのMinecraftServerだけをallocateする

## Consequences

architecture、API、database、CLIがMinecraft operationへ直接最適化できます。将来別programを動かす要件が生じても、evidenceなしにgeneric abstractionへ戻しません。

## Related documents

- [System model](../system-model.md)
- [Minecraft Server domain](../domains/minecraft-server.md)
