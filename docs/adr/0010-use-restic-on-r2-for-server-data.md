# ADR-0010: Use restic repositories on Cloudflare R2 for Server Data

Status: Accepted

## Context

Nodeを交換可能にするにはserver file treeをCompute Instance lifecycleから分離する必要がある。resticはsnapshot、deduplication、integrity、encrypted repository formatを提供する。

## Decision

persistent Minecraft Server operation unitごとにrestic repositoryを持ち、Cloudflare R2のS3-compatible storageへSnapshotを保存する。

## Consequences

repository credential、retention、check/prune、restore verificationを管理する必要がある。空passwordを使用してもrepository read accessに対するsecrecy boundaryとはみなさず、R2 credentialとisolationを主要boundaryとする。

## Related documents

- [Design principles](../design-principles.md)
- [Architecture overview](../architecture/overview.md)
