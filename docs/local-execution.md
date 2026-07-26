# Local execution

## Purpose

The local provider proves the complete control-plane design without a cloud dependency. It is also a useful development and recovery mode.

It is Linux-specific because it uses Unix sockets, Unix signals, `/proc`, rootless Podman, and local child processes.

## Node-agent allocation

For every active ServerInstance, the control plane creates a ComputeInstance row and starts one `mcserver-node-agent` process with:

- ComputeInstance UUID
- random connection token
- loopback control-plane address
- private state directory

The agent connects outward, registers, and then accepts serialized JSON-RPC commands. The control plane passes the same frame limit and a slightly shorter external-operation timeout to the child agent, leaving time for a structured error response before the control-plane RPC watchdog expires. Its versioned durable state file binds the private directory to one ServerInstance UUID and fencing token, rejecting stale commands. State updates use a write-sync-rename-directory-sync sequence so a completed snapshot ID survives control-plane reconnects and ordinary host crashes as reliably as the local filesystem permits.

## Data layout

A local agent directory is created with mode `0700` and contains:

```text
<node-agent-root>/<compute-instance-id>/
├── agent-state.json
└── data/
```

Restore uses a staging directory and rename-based replacement so a failed restore does not intentionally destroy the previous `data/` directory.

A new Server with no source snapshot receives an empty `data/` directory. The system does not create or edit Minecraft configuration files inside it.

## restic

The Server's `data.repository` is passed to restic as `RESTIC_REPOSITORY`. Passwords and backend credentials are inherited from the control-plane environment.

The local agent:

- checks the repository with `restic cat config`
- requires the repository to be initialized explicitly before execution
- restores a specific immutable snapshot ID
- backs up the entire `data` directory
- parses restic JSON Lines output to obtain the resulting snapshot ID

The included local E2E verifier initializes a missing local repository, but refuses to overwrite an existing non-repository path. Each restic invocation waits up to the configured retry-lock duration for repository locks, serializing conflicting operations without treating normal contention as immediate failure. Snapshot publication in SQLite remains separately fenced.

Because rootless Podman maps container UIDs and GIDs into the invoking user's subordinate ID ranges, the host user may not be able to read or remove files created inside `/data` directly. The node agent therefore runs restic restore and backup, as well as instance-data cleanup, through `podman unshare` so all data-plane filesystem operations use the same user namespace as the container.

## Podman

The local agent creates a deterministic container name from the ServerInstance UUID and bind-mounts its private `data/` directory at `/data`.

The system owns only these itzg environment variables:

- `EULA=TRUE`
- `TYPE`
- `VERSION`
- `SKIP_SERVER_PROPERTIES=TRUE`

Additional environment variables are passed through after validation. The container restart policy is `no`; restart decisions remain with reconciliation instead of Podman.

`server.stop` uses the configured Podman stop timeout. After snapshot publication, cleanup removes the container and then the local agent allocation.

## Recovery behavior

- Control-plane restart: node agents keep running, reconnect, and reconciliation resumes.
- Agent restart before `/data` is prepared: safe to recreate and restore again.
- Agent loss after writable `/data` exists and before snapshot publication: treated as data loss; the system does not fall back silently.
- Lost snapshot response: agent state remembers the last snapshot ID, allowing publication after reconnect without another backup.
- Old delayed snapshot: rejected by the ServerInstance fencing token.
