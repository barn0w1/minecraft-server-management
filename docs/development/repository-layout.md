# Planned repository layout

implementation開始時の候補です。現在はdocumentationだけを置き、空crateを先に作りません。

```text
minecraft-server-management/
├─ README.md
├─ AGENTS.md
├─ CONTRIBUTING.md
├─ docs/
├─ crates/
│  ├─ control-plane/
│  ├─ node-agent/
│  ├─ cli/
│  └─ protocol/
├─ deploy/
└─ Cargo.toml
```

## Crate intent

### `control-plane`

Control Plane modular monolith。MinecraftServer、Node、Operation、Snapshot application module、Operator/Agent API、SQLite、Akamai adapter、runtime wiringを含みます。

### `node-agent`

Node Agent modular monolith。HTTP/2 sync、credential、operation journal、Server Runtime、Server Home、RCON、restic capabilityを含みます。

### `cli`

`mcserverctl` binary。Operator API clientとhuman-readable renderingだけを含みます。

### `protocol`

process boundaryで共有するJSON-RPC DTO、method name、ID codec、stable error kindを置きます。Control Plane domain entity、database model、controller traitを共有しません。

## Avoid premature crates

`domain-core`、`provider-api`、`workflow-engine`、`workload-runtime`、`common-utils`のような抽象crateを先に作りません。実際に独立compile boundaryが必要になった時点で抽出します。
