# Minecraft Server Management System

A Rust control plane and node agent for running temporary local or Akamai Cloud Minecraft compute while keeping server data persistent.

The system deliberately treats the Minecraft `/data` directory as opaque. It owns restore, exclusive execution, snapshot publication, and process lifecycle, but it does not parse or rewrite arbitrary Minecraft, mod, plugin, or world files.

## Implemented vertical slices

The local provider executes this complete flow on one Linux host:

```text
client JSON-RPC over Unix socket
  -> Server desired_state = running
  -> reconciler creates one active ServerInstance
  -> local ComputeInstance spawns mcserver-node-agent
  -> node agent restores the Server snapshot with restic, or creates empty /data
  -> node agent starts itzg/minecraft-server with Podman
  -> Server desired_state = stopped
  -> node agent stops Minecraft
  -> node agent creates a restic snapshot
  -> control plane publishes it after checking the fencing token
  -> container and local node-agent resources are removed
  -> ServerInstance is completed
```

Starting the same Server again resolves the new instance from the previously published snapshot.

The Akamai provider adds a second complete compute path:

```text
Server desired_state = running
  -> durable Akamai ComputeInstance and deterministic provider label
  -> Linode API create or adoption after an uncertain response
  -> cloud-init installs and verifies mcserver-node-agent
  -> node agent generates a private key, enrolls over TLS, and reconnects with an issued mTLS certificate
  -> the existing opaque restore, Minecraft, stop, snapshot, and publish flow runs remotely
  -> Linode API deletion removes the ephemeral VM
```

The provider implementation is real, but the repository's acceptance test uses a local fake Linode API so CI never creates billable resources. See [remote provider checkpoint](docs/remote-provider-checkpoint.md) for live-run prerequisites.

The resource model remains small:

- `Server`: durable client-owned desired state and convenient opaque launch/data configuration.
- `ServerInstance`: one reconciler-owned materialization of a Server. At most one is active per Server.
- `ComputeInstance`: one temporary execution allocation backed by `local_process` or an Akamai Cloud VM.
- `Snapshot`: one durable restic snapshot published by an active fenced ServerInstance.

## Workspace

```text
crates/
├── mcserver-control-plane/
├── mcserver-node-agent/
└── mcserver-protocol/
```

- Rust edition: 2024
- pinned toolchain: Rust 1.97.1
- client API: JSON-RPC 2.0 over `/run/mcserver/control-plane.sock`
- agent API: loopback TCP for local agents and direct TLS with private-CA mTLS for remote agents
- persistence: SQLite
- local data snapshots: restic
- local Minecraft execution: rootless Podman and `itzg/minecraft-server`
- operator CLI: `mcserverctl`
- fast integration verifiers: real Rust daemons with deterministic fake Podman/restic and a fake Linode API

## Local prerequisites

Install and configure these in the same user account that runs the control plane:

- Rust 1.97.1
- Podman
- restic
- Python 3 and OpenSSL for the included deterministic E2E verifiers

Confirm that rootless Podman works before testing:

```bash
podman info
restic version
```

Minecraft requires explicit acceptance of its EULA. The API therefore rejects a Server specification unless `accept_eula` is `true`. Only set it after reading and accepting the Minecraft EULA.

## Build and run locally

This branch replaces the experimental migration history with a new initial schema. Remove a database created by an earlier revision before starting this version.

```bash
cargo build --workspace

podman unshare rm -rf -- "$PWD/var/local-agents"
rm -rf -- "$PWD/var"
mkdir -p "$PWD/var"

export RESTIC_PASSWORD='local-development-only'
export MCSERVER_CONTROL_PLANE_SOCKET="$PWD/var/control-plane.sock"
export MCSERVER_CONTROL_PLANE_DATABASE_URL="sqlite://$PWD/var/control-plane.db?mode=rwc"
export MCSERVER_CONTROL_PLANE_AGENT_LISTEN_ADDRESS='127.0.0.1:39001'
export MCSERVER_CONTROL_PLANE_NODE_AGENT_BINARY="$PWD/target/debug/mcserver-node-agent"
export MCSERVER_CONTROL_PLANE_NODE_AGENT_ROOT="$PWD/var/local-agents"
export RUST_LOG='mcserver_control_plane=debug,mcserver_node_agent=info'

target/debug/mcserver-control-plane
```

The control plane passes its environment to local node-agent processes, including restic credentials such as `RESTIC_PASSWORD`, `RESTIC_PASSWORD_FILE`, and backend-specific credentials.

In another terminal, run the two-generation E2E verifier:

```bash
export RESTIC_PASSWORD='local-development-only'
python3 scripts/local_e2e.py \
  --socket "$PWD/var/control-plane.sock" \
  --repository "$PWD/var/restic-repository" \
  --host-port 25565
```

The verifier performs two complete start/stop cycles. It checks Minecraft's TCP port by default, verifies snapshot publication, verifies that the next fencing token increases, and verifies that the second instance uses the first snapshot as its source.

For a faster infrastructure-only check that does not wait for Minecraft to accept TCP connections:

```bash
python3 scripts/local_e2e.py \
  --socket "$PWD/var/control-plane.sock" \
  --repository "$PWD/var/restic-repository" \
  --host-port 25565 \
  --skip-port-check
```

The verifier refuses to start when the selected Podman publish port cannot be bound. On failure it first requests a normal stop and then force-removes only containers carrying this project's managed, local-scope, and Server labels.

For routine development, use the deterministic process-level E2E. It starts the real control-plane and node-agent binaries, but substitutes small fake Podman and restic executables. It also seeds an orphan container, node-agent process, and state directory, then injects one transient Podman and restic failure:

```bash
cargo build --workspace
python3 scripts/deterministic_e2e.py
python3 scripts/remote_provider_e2e.py
```

The remote provider verifier additionally exercises the production mTLS transport, production Akamai HTTP client, and Cloudflare R2 Temporary Credentials API client against local fake APIs. It checks one-time CSR enrollment, exact certificate authentication, prefix-scoped session credentials, lost-create-response adoption, scoped orphan deletion, two generations, and VM deletion without using a real API token or creating a VM:

```bash
python3 scripts/remote_provider_e2e.py
```

The fake API also verifies the production compatibility preflight for `linode/debian13` in `jp-tyo-3`: the image must advertise `cloud-init` and the region must advertise `Metadata` before any create request is accepted.

Basic operation no longer requires hand-written JSON:

```bash
target/debug/mcserverctl --socket "$PWD/var/control-plane.sock" ping
target/debug/mcserverctl --socket "$PWD/var/control-plane.sock" server list
target/debug/mcserverctl --socket "$PWD/var/control-plane.sock" server status SERVER_ID
```

## Validation

The same fast validation runs in `.github/workflows/ci.yml`. The workflow uses the pinned toolchain and does not require Podman or restic because its vertical-slice job uses the deterministic substitutes.

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 -m py_compile scripts/*.py scripts/fakes/*.py
python3 scripts/deterministic_e2e.py
python3 scripts/remote_provider_e2e.py
```

See [architecture](docs/architecture.md), [client API](docs/client-api.md), [local execution](docs/local-execution.md), [development conventions](docs/development.md), [pre-cloud checkpoint](docs/pre-cloud-checkpoint.md), [remote provider checkpoint](docs/remote-provider-checkpoint.md), [production deployment checkpoint](docs/production-deployment-checkpoint.md), and [roadmap](docs/roadmap.md).
