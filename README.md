# Minecraft Server Management System

A Rust control plane and node agent for running temporary Minecraft server compute while keeping server data persistent.

The system deliberately treats the Minecraft `/data` directory as opaque. It owns restore, exclusive execution, snapshot publication, and process lifecycle, but it does not parse or rewrite arbitrary Minecraft, mod, plugin, or world files.

## Implemented local vertical slice

The current code can execute this complete flow on one Linux host:

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

The resource model remains small:

- `Server`: durable client-owned desired state and convenient opaque launch/data configuration.
- `ServerInstance`: one reconciler-owned materialization of a Server. At most one is active per Server.
- `ComputeInstance`: one temporary execution allocation. The implemented provider is `local_process`.
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
- local agent API: separate JSON-RPC 2.0 connection over loopback TCP
- persistence: SQLite
- local data snapshots: restic
- local Minecraft execution: rootless Podman and `itzg/minecraft-server`
- operator CLI: `mcserverctl`
- fast integration verifier: real Rust daemons with deterministic fake Podman/restic

## Local prerequisites

Install and configure these in the same user account that runs the control plane:

- Rust 1.97.1
- Podman
- restic
- Python 3 for the included E2E verifier

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
```

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
```

See [architecture](docs/architecture.md), [client API](docs/client-api.md), [local execution](docs/local-execution.md), [development conventions](docs/development.md), [pre-cloud checkpoint](docs/pre-cloud-checkpoint.md), and [roadmap](docs/roadmap.md).
