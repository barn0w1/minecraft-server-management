# ADR-0008: Do not build stateful provider fakes

Status: Accepted

## Context

provider simulatorはproduction providerの不完全なsecond implementationになり、testがfakeのsemanticsだけを証明する危険がある。

## Decision

Akamai resource lifecycleを再現するstateful fake service/databaseを作らない。pure domain test、SQLite restart test、scripted HTTP transport、Agent protocol harness、read-only integration、bounded real acceptanceを組み合わせる。

## Consequences

real acceptanceにはcost/resource guardが必要になるが、maintenance costとfalse confidenceを減らせる。

## Related documents

- [Design principles](../design-principles.md)
- [Architecture overview](../architecture/overview.md)
