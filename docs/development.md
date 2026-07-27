# Development conventions

## Naming

Follow standard Rust conventions:

- packages: `kebab-case`
- modules, functions, variables, SQL columns, and JSON fields: `snake_case`
- types, traits, and variants: `UpperCamelCase`
- constants: `SCREAMING_SNAKE_CASE`
- identifiers: singular resource name plus `Id`
- JSON-RPC methods: `resource.verb`

Project terms:

- `Server`: durable client-facing desired state plus convenient opaque configuration.
- `ServerInstance`: one materialization; do not call it a runtime.
- `ComputeInstance`: one temporary execution allocation.
- `Snapshot`: one durable opaque data generation.
- `generation`: client-controlled desired-state revision.
- `fencing_token`: monotonically increasing Server-scoped writer token.

## Module layout

Use the file-plus-directory module layout and do not add `mod.rs`:

```text
application.rs
application/
└── server_service.rs
```

The root file declares child modules and explicitly re-exports the public surface.

## Boundaries

- Domain code does not depend on wire DTOs, SQLx, Tokio, Podman, or restic.
- `mcserver-protocol` owns wire types only, not business rules.
- Transport handlers delegate to application services or infrastructure adapters.
- External command details stay in the node-agent executor.
- `/data` remains opaque.
- Reconciler-owned resources are read-only through the client API.
- Add a resource only when it has a genuinely independent lifecycle, identity, sharing model, or API.

## State and reconciliation

- The database is authoritative; queue sends are best effort.
- External operations must be idempotent or recoverable after uncertain responses. Provider creates use deterministic identity and discovery before retrying.
- Persist observations, not one combined global phase.
- Isolate a Server's reconciliation error from the daemon and other Servers.
- Enforce concurrency invariants in SQLite as well as in code.
- Validate fencing tokens before publishing authoritative data.

## Time

- persistent wall clock: `UnixTimestampMillis`
- SQL/JSON representation: non-negative integer milliseconds since Unix epoch
- names: `*_at_ms`
- durations/deadlines/backoff: `Duration` and monotonic timers
- create timestamps at the boundary where the corresponding fact is observed
- preserve chronological ordering when persisting after wall-clock rollback

## SQL

Use SQL literals or `&'static str` constants with bind parameters. Use `QueryBuilder` only when SQL structure must be dynamic. Do not use `AssertSqlSafe` merely to bypass SQLx auditing.

Schema constraints must mirror domain invariants. Migration files are immutable after release; during the current pre-release phase, this repository may intentionally replace the experimental schema when the change is documented.

## External processes

- use argument APIs, never shell interpolation
- set `kill_on_drop(true)` for bounded command execution
- capture stdout/stderr and include a bounded diagnostic on failure
- use deterministic resource names and complete ownership labels; revalidate ownership before destructive provider calls
- scope local runtime resources so cleanup never targets another control-plane installation
- verify an untracked PID still belongs to the intended local agent before signaling it
- treat timeout responses as uncertain and do not reuse a desynchronized RPC session

## Daemon lifecycle

Long-running tasks must observe a shared cancellation token, stop accepting new work, drain safely, and be supervised. Durable consistency must not depend on abrupt process termination.

Control-plane shutdown deliberately leaves active local agents and Minecraft containers running so they can reconnect. Explicit Server desired state controls Minecraft shutdown.

## Git

Use reviewable imperative Conventional Commit subjects where practical. Author and committer identity:

```text
barn0w1 <yuito.kiuchi.dev@gmail.com>
```

## External command conventions

- Node-agent restic commands use `--retry-lock`; `MCSERVER_NODE_AGENT_RESTIC_RETRY_LOCK_SECONDS` defaults to 300 seconds.
- Any operation that reads, writes, restores, or removes container-owned server data runs through `podman unshare`; host-side recursive filesystem operations must not assume subordinate-ID-owned paths are accessible.
- Command timeout and repository-lock wait are durations, never persisted wall-clock timestamps.


## Test layers

Use the cheapest layer that proves the change:

1. unit tests for domain rules, parsers, and retry calculations
2. `scripts/deterministic_e2e.py` for daemon, transport, persistence, reconciliation, retry, and cleanup behavior
3. `scripts/local_e2e.py --skip-port-check` for real Podman/restic infrastructure
4. `scripts/remote_provider_e2e.py` for TLS enrollment, the real HTTP adapter, uncertain-response recovery, and remote lifecycle
5. `scripts/local_e2e.py` for actual Minecraft readiness and two-generation data recovery

Fake executables must model command-line contracts and persistent observations, not reimplement production business rules.
## External HTTP providers

- Use an official API specification as the contract; keep provider DTOs inside the adapter.
- Configure a bounded request timeout and preserve structured provider errors.
- Treat `429` as scheduling information and honor a valid `Retry-After`.
- Search by deterministic identity before create and after uncertain create responses.
- Treat not-found delete as converged absence.
- Never delete using a provider ID alone; verify the expected label and ownership scope.
- Redact bearer tokens and per-compute credentials from `Debug` output.
- Allow plaintext HTTP only for an explicitly loopback test endpoint.

## Node data ownership

`MCSERVER_NODE_AGENT_DATA_ACCESS_MODE` accepts `auto`, `podman_user_namespace`, or `host`. `auto` chooses host access for an effective UID of zero and Podman user-namespace access otherwise. Local rootless execution should use the namespace mode; the remote root systemd bootstrap sets host mode explicitly. Restic, directory swaps, and recursive cleanup must all use the selected mode.
