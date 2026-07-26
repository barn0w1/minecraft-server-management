# Roadmap

## Completed: local vertical slice

- durable Server desired state and optimistic generation checks
- reconciler-owned ServerInstance and ComputeInstance with active uniqueness
- fencing-token-protected restic snapshot publication
- loopback node-agent registration, reconnect, and durable operation state
- rootless Podman Minecraft lifecycle with subordinate-ID-safe data operations
- real two-generation Fedora/Podman/restic/Minecraft E2E

## Completed: pre-cloud reliability checkpoint

- managed-resource labels and installation-local scope
- startup orphan container, process, and state-directory reaping
- TCP port preflight and label-scoped failed-test cleanup
- deterministic fake Podman and restic command adapters
- process-level two-generation E2E using the real daemons
- injected transient Podman and restic failure recovery
- reusable Unix-socket JSON-RPC client
- `mcserverctl` operator commands
- aggregate read-only `server.status`

See [the checkpoint definition](pre-cloud-checkpoint.md).

## Next checkpoint: remote Akamai vertical slice

Implement this as one coherent capability rather than separately deployable insecure steps:

1. remote agent TLS server identity and one-time enrollment
2. short-lived ComputeInstance-bound agent credentials and revocation
3. provider-neutral compute boundary extracted from the proven local implementation
4. Akamai Cloud create, inspect, and delete with deterministic provider labels
5. recovery after create/delete responses are lost or time out
6. cloud-init or image-based node-agent installation
7. systemd/Quadlet Minecraft supervision on the ephemeral node
8. R2-compatible restic repository credential delivery
9. cloud orphan discovery, rate-limit handling, and bounded retry
10. full remote start, restore, readiness, stop, snapshot, publish, and VM deletion E2E

## Following checkpoints

- audit/event history and metrics
- snapshot listing, explicit rollback, retention, prune, and repository checks
- Unix-socket authorization and remote authenticated client gateway
- Discord bot built on the same client API
- disaster-recovery and production operations documentation

Continue to prefer complete vertical capabilities over generic frameworks. Add resources only for independent lifecycles, and extract provider traits when the second implementation supplies concrete requirements.
