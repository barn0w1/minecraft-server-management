# Initial architecture

## Design boundary

A `Server` is the durable, client-facing aggregate that says:

> Run this opaque server data with this minimum execution and compute configuration.

The aggregate is intentionally convenient rather than ontologically pure. Data, compute configuration, and process configuration may become separate resources later only when they gain an independent lifecycle, sharing model, or API.

The system does not parse or manage arbitrary files under the Minecraft server data directory. Humans remain responsible for Minecraft-specific configuration consistency.

## Resource direction

The initial resource model is deliberately small:

- `Server`: durable desired state, data reference, and minimum launch configuration.
- `ServerInstance`: one reconciler-owned materialization record for a `Server`.
- `ComputeInstance`: a temporary VM with its own lifecycle; planned.
- `Snapshot`: a durable generation of opaque server data; planned.

At most one active `ServerInstance` may exist for a `Server`. SQLite enforces this with a partial unique index rather than relying on an application-level precondition.

Each instance snapshots the source Server generation and resolved specification. A per-Server fencing token increases for every new instance. Future data and agent operations must present that token so stale instances cannot publish authoritative results.

## Reconciliation

Clients mutate desired state. They do not execute a long imperative workflow through one RPC call.

The control plane reacts immediately to resource changes and periodically resynchronizes all servers. Reconcilers observe durable state and apply at most one idempotent transition at a time, repeating until the resource is locally stable.

Current behavior is intentionally narrow:

- desired `running` with no active instance creates one instance
- desired `stopped` requests stop on the active instance
- a stop-requested instance is marked completed

Termination currently completes immediately because compute and node-agent operations do not exist yet. Later reconcilers will replace that placeholder transition with durable observed facts. No single linear lifecycle or global state-machine enum represents the whole system.

## Time model

Persistent timestamps and API event timestamps use signed 64-bit milliseconds since the Unix epoch. Domain code wraps them in `UnixTimestampMillis`; database and JSON field names use the `_at_ms` suffix.

Wall-clock timestamps are never used for retry intervals, timeouts, or elapsed-time measurement. Those use `Duration` and Tokio's monotonic timer facilities.

## Process lifecycle

The control plane listens for both `SIGINT` and `SIGTERM`. Shutdown is cooperative:

1. stop accepting new Unix socket connections
2. allow an in-flight JSON-RPC request to finish
3. stop reconciliation after its current operation
4. wait for connection and worker tasks up to the configured timeout
5. close the SQLite pool and remove the Unix socket path

An unexpected exit of either core service terminates the daemon instead of leaving a partially functioning process.

## Interfaces

There are two separate JSON-RPC interfaces:

1. Client API: human tools, CLI programs, and bots connect to the control plane over a local Unix socket.
2. Agent API: the control plane communicates with remote node agents over a network transport that has not yet been selected.

These interfaces may share JSON-RPC envelope code, but not method namespaces, DTOs, authentication, framing, or versioning policy.
