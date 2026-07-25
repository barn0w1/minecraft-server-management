# ADR-0009: Use Podman, Quadlet, and systemd for the Server Runtime

Status: Accepted

## Context

Minecraft Server processにはcontainer isolation、declarative configuration、boot integration、restart supervision、inspectable local stateが必要です。

## Decision

Node上のServer RuntimeにPodmanを使用し、container definitionをQuadletとしてmaterializeし、systemdでlifecycleをsuperviseします。

v1のcontainerはitzg/minecraft-serverだけです。generic Workload resourceやarbitrary container specificationは提供しません。

## Consequences

Node AgentはQuadlet file生成、atomic replacement、daemon reload、unit lifecycle、Podman/systemd observationをtyped adapterとして実装します。Control Planeへraw commandやunit contentを露出させません。

## Related documents

- [Minecraft Server domain](../domains/minecraft-server.md)
- [Module boundaries](../architecture/module-boundaries.md)
