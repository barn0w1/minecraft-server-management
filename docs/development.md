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

JSON-RPC methods use `resource.verb` in lower snake case, for example `server.set_desired_state`. JSON fields use `snake_case`.

Database tables use plural `snake_case`. Foreign-key columns use the singular resource name plus `_id`.

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

## State modeling

Do not create one phase enum that combines Server, ServerInstance, ComputeInstance, agent connection, data operations, and process state.

Prefer independent durable facts and conditions. Reconcilers compare desired state with observed facts and request idempotent operations.

## Git history

Commits should be small enough to review and use imperative Conventional Commit subjects where practical. Repository commits use:

```text
barn0w1 <yuito.kiuchi.dev@gmail.com>
```
