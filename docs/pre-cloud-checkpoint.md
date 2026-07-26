# Pre-cloud checkpoint

This checkpoint is the stable boundary immediately before adding remote node enrollment and an Akamai Cloud compute provider.

## Acceptance criteria

The real Fedora/rootless-Podman/restic/Minecraft two-generation E2E was proven before this checkpoint. The checkpoint is accepted when the updated workspace also passes:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
python3 scripts/deterministic_e2e.py
```

The deterministic E2E must prove that:

- the real control-plane and node-agent processes complete two generations without a real container or world
- transient external-command failures converge through bounded per-Server retry
- startup cleanup removes scoped resources absent from the authoritative database
- a failed E2E requests desired-state cleanup and removes any remaining label-scoped test container
- `server.status` and `mcserverctl` expose operational state without making reconciler-owned resources mutable

## Stable boundaries

The next provider must preserve these contracts:

- `Server` remains the client-owned durable desired state
- `ServerInstance` remains the fenced unit of writable data ownership
- `ComputeInstance` remains one replaceable execution allocation
- `/data` remains opaque and is published only through an immutable snapshot
- uncertain external responses are reconciled by provider identity, never by blindly creating another resource
- queue delivery is an optimization; the database and periodic resync remain authoritative

## Intentionally deferred to the cloud checkpoint

- remote agent TLS identity and one-time enrollment
- provider-neutral compute allocation backed by Akamai Cloud
- cloud-init or image-based node-agent installation
- systemd/Quadlet supervision on an ephemeral node
- object-storage credentials and R2-backed restic repositories
- cloud orphan discovery, API rate limiting, and uncertain-create recovery

Those items should be implemented together as one remote vertical slice. Exposing the current loopback token protocol or direct-Podman local executor over a public network is not an acceptable intermediate deployment.
