# Development conventions

## Naming

Rust naming follows the standard conventions:

- crates and packages: `kebab-case`
- modules, functions, variables, and database columns: `snake_case`
- types, traits, and enum variants: `UpperCamelCase`
- constants: `SCREAMING_SNAKE_CASE`
- resource identifiers: singular resource name plus `Id`, such as `ServerId`

Project terminology is intentionally narrow:

- `Server`: the durable client-facing aggregate containing desired state and opaque data/configuration references.
- `ServerInstance`: one materialization of a `Server`; it is not a VM and it is not called a runtime.
- `ComputeInstance`: one provider VM.
- `Snapshot`: one durable generation of opaque server data.
- `generation`: the version of client-controlled desired state.
- `observed_generation`: reserved for status written by a reconciler.
- `fencing_token`: a monotonically increasing token that rejects stale writers.

JSON-RPC methods use `resource.verb` in lower snake case, for example `server.set_desired_state`. JSON fields use `snake_case`.

Database tables use plural `snake_case`. Foreign-key columns use the singular resource name plus `_id`.

## Time and duration

Use one wall-clock representation throughout the system:

- Rust domain type: `UnixTimestampMillis`
- persisted representation: SQLite `INTEGER`
- wire representation: JSON integer
- unit: milliseconds since `1970-01-01T00:00:00Z`
- timestamp field suffix: `_at_ms` in SQL and JSON

Use `Duration` for intervals, deadlines, retry delays, and shutdown timeouts. Do not subtract wall-clock timestamps to measure elapsed time and do not store local time-zone values.

## Module layout

For modules with child modules, use the modern file-plus-directory layout:

```text
application.rs
application/
└── server_service.rs
```

Do not introduce `mod.rs`. The named root file declares child modules and defines the module's public surface through explicit re-exports.

## Boundaries

- Domain code must not depend on JSON-RPC wire DTOs.
- Protocol crates contain wire types only and must not own business rules.
- Unix socket handling must not contain persistence or domain decisions.
- Cloud, systemd, Podman, restic, and object-storage details belong behind infrastructure boundaries.
- Minecraft server data remains opaque. Do not add file-specific behavior without an explicit decision.
- Reconciler-owned resources are read-only through the client API unless a clear client-owned field is introduced.

## SQL construction

Pass fixed SQL to SQLx as string literals or `&'static str` constants, and pass values only through bind parameters. Use `QueryBuilder` when the SQL structure genuinely must be assembled at runtime. Do not use `AssertSqlSafe` merely to silence the type system; it requires an explicit security review of the generated SQL.

## State modeling

Do not create one phase enum that combines Server, ServerInstance, ComputeInstance, agent connection, data operations, and process state.

Prefer independent durable facts and conditions. Reconcilers compare desired state with observed facts and request idempotent operations.

Database constraints enforce invariants that must survive concurrent workers, including the maximum of one active `ServerInstance` per `Server`.

## Daemon lifecycle

Core services must participate in cooperative shutdown. New long-running tasks must:

- observe the shared shutdown signal
- stop accepting new work
- finish or safely abandon the current idempotent operation
- be supervised so unexpected termination stops the daemon
- not depend on process termination for durable consistency

## Git history

Commits should be small enough to review and use imperative Conventional Commit subjects where practical. Repository commits use:

```text
barn0w1 <yuito.kiuchi.dev@gmail.com>
```
