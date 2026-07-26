# Roadmap

## Completed: foundation and local vertical slice

- Rust workspace, edition 2024, Rust 1.97.1
- Unix-socket client JSON-RPC
- durable Server desired state with optimistic generation checks
- reconciler-owned ServerInstance with active uniqueness and fencing
- graceful control-plane shutdown and task supervision
- loopback node-agent JSON-RPC with reconnect and registration token
- local-process ComputeInstance provider
- opaque restic restore and snapshot
- direct rootless Podman execution of `itzg/minecraft-server`
- fenced transactional snapshot publication
- two-generation local E2E verifier

## Next: harden local operation

- run and fix `fmt`, `check`, `clippy`, and tests on the target Rust toolchain
- execute the E2E verifier against real Podman/restic/Minecraft
- add integration tests with fake Podman and restic executables
- persist bounded operation attempts and improve per-Server retry backoff
- expose read-only ComputeInstance and snapshot diagnostics if operationally needed
- add a small `mcserverctl` client instead of relying on raw JSON
- add structured metrics and an audit/event log

## Cloud node provider

- provider-neutral compute adapter
- Akamai Cloud instance create, inspect, and delete
- deterministic provider labels for idempotency after uncertain API responses
- cloud-init or image-based node-agent installation
- short-lived enrollment credentials and rotation
- orphan instance discovery and cleanup
- API rate-limit handling and bounded backoff

## Production node execution

- replace direct Podman lifecycle with systemd and Quadlet where appropriate
- preserve the existing agent protocol and opaque data boundary
- explicit filesystem ownership and SELinux policy
- node boot recovery and service supervision
- log streaming and bounded retention

## Object storage and data operations

- restic repository on an S3-compatible backend such as Cloudflare R2
- credential delivery that does not place long-lived secrets in Server specs
- retention policy and garbage collection
- scheduled repository integrity checks
- snapshot listing and deliberate rollback API
- disaster-recovery documentation

## Multi-client operation

- Unix-socket authorization by owner/group
- `mcserverctl`
- Discord bot using the same client API
- optional remote authenticated client gateway
- audit identity and idempotency keys for mutating requests

The roadmap should continue to prefer complete vertical capabilities over broad generic abstractions. New resources are introduced only when the existing model can no longer express an independent lifecycle safely.
