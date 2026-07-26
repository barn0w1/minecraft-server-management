# Implementation roadmap

## Milestone 0: repository foundation

- Rust workspace and pinned toolchain
- Separate control-plane, node-agent, and wire-protocol crates
- Naming and module conventions

## Milestone 1: durable Server desired state

- SQLite schema and migrations
- Unix socket JSON-RPC server
- `system.ping`
- `server.create`, `server.get`, `server.list`
- `server.set_desired_state`
- optimistic generation checks
- reconciliation scheduling boundary

## Milestone 2: ServerInstance reconciliation

- durable `ServerInstance` records
- database-enforced maximum of one active instance per `Server`
- resolved copy of the source `Server` generation
- stop intent and terminal result represented as independent facts
- fencing token for writable data ownership

## Milestone 3: ComputeInstance provider

- provider-neutral compute contract
- Akamai Cloud implementation
- durable create, inspect, and delete operations
- idempotency and recovery after uncertain provider responses

## Milestone 4: node-agent transport

- explicit network trust and enrollment model
- separate agent JSON-RPC namespace
- durable command IDs and idempotent command handling
- reconnect and stale-agent rejection

## Milestone 5: opaque data operations

- restore and snapshot operations
- restic repository integration
- object storage integration
- safe handoff of the authoritative writable copy

## Milestone 6: Minecraft process execution

- systemd, Podman, and Quadlet integration
- minimal `itzg/minecraft-server` environment configuration
- readiness, stop, and crash observation
