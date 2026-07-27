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

See [the pre-cloud checkpoint definition](pre-cloud-checkpoint.md).

## Implemented: remote Akamai provider checkpoint

- provider-neutral compute dispatch preserving the proven local provider
- real Akamai/Linode API v4 HTTP adapter for create, list, get, and delete
- image and region capability preflight before billable creation
- deterministic provider labels plus managed and installation-scope tags
- ownership verification before adoption or deletion
- recovery after a provider-side create succeeds but its response is lost
- structured API errors, pagination, `X-Filter`, request timeouts, and `Retry-After`
- opt-in scoped Akamai orphan adoption/deletion during startup
- separate remote TLS agent listener with server-name and CA verification
- one-time cloud-init enrollment token rotated into a persisted reconnect token
- cloud-init installation with HTTPS download and SHA-256 verification
- systemd supervision of the remote node agent
- credential delivery boundary for restic and R2-compatible object storage
- deterministic remote E2E using the real daemons, rustls transport, and reqwest adapter

See [the remote provider checkpoint definition](remote-provider-checkpoint.md).

The implementation checkpoint is accepted only after the workspace and both deterministic E2Es pass. A live Akamai deployment is deliberately separate because it is billable and requires operator-owned DNS, TLS, firewall, API, binary-distribution, and object-storage credentials.

## Next checkpoint: live remote vertical slice

1. refresh and commit `Cargo.lock` with the pinned Rust toolchain
2. deploy the control plane on the intended persistent VM
3. validate public DNS, TLS chain, and remote listener reachability
4. publish a target-compatible node-agent binary and verify its SHA-256
5. create a dedicated least-privilege Akamai API token and firewall
6. initialize the R2-backed restic repository with isolated credentials
7. run one smallest-instance two-generation Minecraft lifecycle
8. interrupt create, control-plane, agent, stop, and delete paths to verify recovery
9. inspect provider billing and orphan state before enabling startup reaping
10. validate and document the Debian 13 (`linode/debian13`) / Tokyo 3 (`jp-tyo-3`) production profile, systemd, Podman, and network assumptions

## Following checkpoints

- replace direct remote Podman execution with Quadlet after the target image is validated
- optional mutual TLS and external secret-manager integration
- audit/event history and metrics
- snapshot listing, explicit rollback, retention, prune, and repository checks
- Unix-socket authorization and remote authenticated client gateway
- Discord bot built on the same client API
- disaster-recovery and production operations documentation

Continue to prefer complete vertical capabilities over generic frameworks. Add resources only for independent lifecycles, and extract provider traits from concrete implementations rather than speculative abstractions.
