# Minecraft Server Management System

A Rust control plane and node agent for running temporary Minecraft server compute while keeping server data persistent.

The project deliberately treats the contents of a Minecraft server data directory as opaque. The system manages ownership, snapshots, compute allocation, and process execution without trying to understand or rewrite every Minecraft, mod, or plugin configuration file.

## Current foundation

- Rust 2024 edition, pinned to Rust 1.97.1
- `mcserver-control-plane`: persistent desired-state API over JSON-RPC 2.0 on a Unix socket
- `mcserver-node-agent`: daemon boundary reserved for remote node execution
- `mcserver-protocol`: wire-only JSON-RPC types
- SQLite persistence for the first control-plane resource: `Server`

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

See [`docs/architecture.md`](docs/architecture.md) and [`docs/roadmap.md`](docs/roadmap.md).
