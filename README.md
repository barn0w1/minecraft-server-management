# Minecraft Server Management System

A Rust control plane and node agent for running temporary Minecraft server compute while keeping server data persistent.

The project deliberately treats the contents of a Minecraft server data directory as opaque. The system manages ownership, snapshots, compute allocation, and process execution without trying to understand or rewrite every Minecraft, mod, or plugin configuration file.

## Current foundation

- Rust 2024 edition, pinned to Rust 1.97.1
- `mcserver-control-plane`: persistent desired-state API over JSON-RPC 2.0 on a Unix socket
- `mcserver-node-agent`: daemon boundary reserved for remote node execution
- `mcserver-protocol`: wire-only JSON-RPC types
- SQLite persistence for durable `Server` and reconciler-owned `ServerInstance` resources
- Cooperative `SIGINT`/`SIGTERM` shutdown with bounded task draining

The control-plane client socket defaults to:

```text
/run/mcserver/control-plane.sock
```

Each socket frame is one complete JSON value followed by a newline. A frame may contain one JSON-RPC request or a JSON-RPC batch.

## Workspace

```text
crates/
├── mcserver-control-plane/
├── mcserver-node-agent/
└── mcserver-protocol/
```

See [`docs/architecture.md`](docs/architecture.md), [`docs/development.md`](docs/development.md), and [`docs/roadmap.md`](docs/roadmap.md).

## Local development configuration

The production defaults use `/run/mcserver` and `/var/lib/mcserver`. For an unprivileged local run, override them:

```bash
mkdir -p var
export MCSERVER_CONTROL_PLANE_SOCKET="$PWD/var/control-plane.sock"
export MCSERVER_CONTROL_PLANE_DATABASE_URL="sqlite://$PWD/var/control-plane.db?mode=rwc"
export MCSERVER_CONTROL_PLANE_SHUTDOWN_TIMEOUT_SECONDS=15
export RUST_LOG=mcserver_control_plane=debug
cargo run -p mcserver-control-plane
```

Example request frame:

```json
{"jsonrpc":"2.0","method":"system.ping","id":1}
```

Create a durable server resource:

```json
{"jsonrpc":"2.0","method":"server.create","params":{"name":"community","spec":{"compute":{"region":"jp-osa","instance_type":"g6-standard-2","image":"debian-13"},"process":{"container_image":"docker.io/itzg/minecraft-server:latest","server_type":"VANILLA","version":"LATEST","environment":{}},"data":{"repository":"r2:mcserver/community"}}},"id":2}
```

Set desired state using an optional optimistic generation check:

```json
{"jsonrpc":"2.0","method":"server.set_desired_state","params":{"server_id":"00000000-0000-0000-0000-000000000000","desired_state":"running","expected_generation":1},"id":3}
```

The reconciler materializes at most one active `ServerInstance` for a running Server and preserves instance history with a fencing token. `server_instance.get` and `server_instance.list` expose that read-only state.
