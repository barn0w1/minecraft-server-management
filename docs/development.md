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
- External operations must be idempotent or recoverable after uncertain responses.
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
- use deterministic resource names and provider labels
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
- Command timeout and repository-lock wait are durations, never persisted wall-clock timestamps.
