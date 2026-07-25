# Planned repository layout

実装開始時の候補です。現在はdocumentationだけを置き、空crateを先に作りません。

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

Control Plane modular monolith。domain module、application service、interface、infrastructure adapter、runtime wiringを含みます。

### `node-agent`

Node Agent modular monolith。Agent CoreとNode/Workload/Server Data/Minecraft capabilityを含みます。

### `cli`

`mcserverctl` binary。Operator API clientだけを含みます。

### `protocol`

process boundaryで共有するwire DTO、method name、error kind、ID codecだけを置きます。Control Planeのdomain entity、database model、controller traitを共有しません。

## Avoid premature crates

`domain-core`、`provider-api`、`workflow-engine`、`common-utils`のような抽象crateを先に作りません。実際に独立compile boundaryが必要になった時点で抽出します。
