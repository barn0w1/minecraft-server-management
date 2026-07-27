# Architecture

## Boundary

A `Server` is the durable, client-facing aggregate that says:

> Run this opaque Minecraft data with this minimum process and compute configuration.

The aggregate is intentionally convenient rather than ontologically pure. Data and launch settings remain embedded until they require an independent lifecycle, sharing model, or client API.

The system does not interpret arbitrary files under `/data`. Humans remain responsible for Minecraft-specific configuration consistency.

## Resources

### Server

Client-owned and durable. It contains:

- `desired_state`: `running` or `stopped`
- local or Akamai compute selection
- minimum itzg container settings
- opaque restic repository reference
- current authoritative snapshot ID
- optimistic `generation`

Changing desired state returns after the database commit. It does not wait for external work.

### ServerInstance

Reconciler-owned and durable. It represents one materialization of a Server and records:

- the source Server generation and resolved specification
- the source and result snapshot IDs
- a monotonically increasing Server-scoped fencing token
- independent observations for data preparation and process execution
- stop intent, errors, and terminal result

SQLite enforces at most one active ServerInstance per Server.

### ComputeInstance

Reconciler-owned and temporary. The `local_process` provider maps one ComputeInstance to one `mcserver-node-agent` child process and its private state directory. The `akamai` provider maps it to one Linode API instance with a deterministic label, managed/scope tags, provider ID, and observed public IPv4 address.

ComputeInstance and ServerInstance lifecycles are separate. A ComputeInstance can disappear or be recreated while a ServerInstance remains active, provided writable data has not been lost.

### Snapshot

A restic snapshot is published only by the active ServerInstance holding the matching fencing token. Publication updates the instance result and `Server.current_snapshot_id` in one SQLite transaction.

## Desired state and reconciliation

The database is the source of truth. Queue notifications only reduce latency; periodic resynchronization recovers missed notifications and process restarts.

Each reconcile step performs at most one idempotent transition and then observes again. There is no global phase enum combining Server, instance, compute, agent, data, and process state.

Running converges through these facts:

```text
active ServerInstance exists
active provider-specific ComputeInstance exists
node agent is connected
/data is prepared from source_snapshot_id or empty initialization
Minecraft container is observed running
```

Stopping converges through these facts:

```text
stop intent recorded
Minecraft container observed stopped
restic snapshot created and fenced publication committed
container removed
node agent and ComputeInstance removed
ServerInstance completed
```

A reconcile failure is isolated to its Server, stored as `last_error`, and retried. It does not stop the daemon.

## Data authority

```text
Server stopped:
  Server.current_snapshot_id is authoritative.

Server running:
  the active fenced ServerInstance's /data is the only writable copy.

Stop completed:
  the newly published snapshot becomes authoritative.
```

If the control plane loses the only compute holding prepared writable data before a result snapshot is published, it refuses to silently recreate from an older snapshot and reports writable-data loss.

## Interfaces

Two JSON-RPC interfaces are intentionally separate:

1. Client API: local human tools and bots use a Unix socket.
2. Agent API: node agents initiate a network connection to the control plane.

They share the JSON-RPC envelope only. Their DTOs, methods, framing, trust boundary, and versioning are separate.

The local agent listener binds only to a loopback address. The remote listener is separate and requires TLS with a configured CA and server name. Connections are always initiated by the node agent. A listener accepts only the expected provider, so an Akamai credential cannot register through the local boundary.

Local agents use a per-Compute reconnect token directly. Akamai cloud-init receives a separate one-time enrollment token. The first TLS registration returns the stable reconnect token and closes; the agent persists it mode `0600` and reconnects immediately. A successful reconnect clears the enrollment token from SQLite. Registration always requires an active ComputeInstance and matching provider.

The client API also has a reusable Unix-socket client and `mcserverctl`. `server.status` is a read-only projection combining the durable Server, its active instance and compute allocation, and current agent connectivity; it does not introduce another persisted phase or resource.

## Time model

Persistent wall-clock timestamps use a single representation:

- domain: `UnixTimestampMillis`
- SQLite: non-negative `INTEGER`
- JSON: non-negative integer
- unit: milliseconds since the Unix epoch
- field suffix: `_at_ms`

Timeouts, backoff, polling, and elapsed time use `Duration` and monotonic timers. Repository updates preserve per-resource chronological ordering if the wall clock moves backwards.

## Process shutdown

The control plane handles `SIGINT` and `SIGTERM` cooperatively:

1. stop accepting client and agent connections
2. stop reconciliation after its current idempotent operation
3. drain supervised tasks up to a configured deadline
4. close SQLite and remove the Unix socket

Control-plane shutdown does not intentionally stop active Minecraft containers or local agents. The agents reconnect after the control plane restarts, allowing reconciliation to continue from durable state.

Local Podman containers carry managed, installation-scope, Server, ServerInstance, and ComputeInstance labels. Each local allocation also has control-plane-owned runtime metadata containing its ComputeInstance ID, local scope, PID, Linux boot ID, and process start time. On startup the control plane compares container labels and state directories with active database rows, verifies saved process identity without reading another process's environment, then removes only scoped orphan resources. The boot ID and start time prevent a reused PID from being signaled. This cleanup is recovery from lost local bookkeeping, not a replacement for normal desired-state shutdown.
## Akamai provider reconciliation

The provider boundary is selected by the resolved `ServerInstance` specification and by the persisted `ComputeInstance.provider`; deletion never depends on the current mutable Server configuration.

Akamai ownership is established by all three facts:

```text
provider label = mcserver-<ComputeInstance UUID>
managed tag    = mcserver-managed
scope tag      = mcserver-scope-<installation scope>
```

Create checks the exact label before issuing `POST`. This recovers a provider-side success whose HTTP response was lost. GET and DELETE revalidate label and tags before controlling a persisted provider ID. `429 Retry-After` participates in reconcile scheduling. Startup orphan deletion is opt-in and restricted to the configured scope.

The cloud-init bootstrap installs the node agent as a systemd service, verifies the downloaded binary SHA-256, and supplies opaque restic/object-storage credentials through an operator-owned environment file. Minecraft remains managed by the same node-agent executor used by the proven local slice; provider code never interprets `/data`.

## Node data ownership boundary

Node-agent data operations use one explicit ownership boundary. Local rootless processes operate through `podman unshare`; remote root systemd agents operate directly on the host filesystem. Restic, restore swaps, and cleanup never mix these modes within one agent process.
