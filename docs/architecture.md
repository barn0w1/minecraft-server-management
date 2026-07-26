# Initial architecture

## Design boundary

A `Server` is the durable, client-facing aggregate that says:

> Run this opaque server data with this minimum execution and compute configuration.

The aggregate is intentionally convenient rather than ontologically pure. Data, compute configuration, and process configuration may become separate resources later only when they gain an independent lifecycle, sharing model, or API.

The system does not parse or manage arbitrary files under the Minecraft server data directory. Humans remain responsible for Minecraft-specific configuration consistency.

## Resource direction

The initial resource model is deliberately small:

- `Server`: durable desired state, data reference, and minimum launch configuration.
- `ServerInstance`: one materialization of a `Server`; planned, but not implemented in the first milestone.
- `ComputeInstance`: a temporary VM with its own lifecycle; planned.
- `Snapshot`: a durable generation of opaque server data; planned.

At most one active `ServerInstance` may own a `Server`'s writable data. This must eventually be enforced by a database constraint and a fencing token, not only by application code.

## Reconciliation

Clients mutate desired state. They do not execute a long imperative workflow through one RPC call.

The control plane reacts immediately to resource changes and periodically resynchronizes unfinished resources. Reconcilers observe durable state and request the next idempotent action. No single linear lifecycle or global state-machine enum represents the entire system.

## Interfaces

There are two separate JSON-RPC interfaces:

1. Client API: human tools, CLI programs, and bots connect to the control plane over a local Unix socket.
2. Agent API: the control plane communicates with remote node agents over a network transport that has not yet been selected.

These interfaces may share JSON-RPC envelope code, but not method namespaces, DTOs, authentication, framing, or versioning policy.
