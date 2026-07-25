# ADR-0009: Use Podman, Quadlet, and systemd for the Workload Runtime

Status: Accepted

## Context

managed NodeはGNU/Linuxであり、systemdのrestart、dependency、logging、boot integrationを利用できる。full container orchestratorを導入せずdeclarativeなunitを扱える。

## Decision

initial Workload RuntimeはPodman containerをQuadlet definitionとして管理し、generated systemd serviceをsystemdでsuperviseする。

## Consequences

Node AgentはQuadlet/systemd detailをtyped Workload operationへ隠蔽する。exact rootful/rootless model、directory、resource limitはWorkload milestoneで決定する。

## Related documents

- [Design principles](../design-principles.md)
- [Architecture overview](../architecture/overview.md)
