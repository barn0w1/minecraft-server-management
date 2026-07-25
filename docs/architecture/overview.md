# Architecture overview

## System shape

Minecraft Server Management Systemは、二つのmodular monolithと複数のexternal systemから構成されます。

```text
Operator clients
    │
    ▼
Control Plane
    │
    ├─ Minecraft Server
    ├─ Server Data
    ├─ Workload
    └─ Node ───────────────> Akamai Cloud
    │
    │ authenticated Agent protocol
    ▼
Node Agent
    │
    ├─ Minecraft Server ───> Management Protocol / RCON
    ├─ Server Data ────────> restic / filesystem / R2
    ├─ Workload Runtime ───> Podman / Quadlet / systemd
    └─ Node ───────────────> GNU/Linux / systemd
```

## Dependency direction

```text
MinecraftServerController
    ├─ uses Minecraft Server operations
    ├─ uses Server Data operations
    ├─ uses Workload operations
    └─ uses Node lifecycle

Workload
    └─ requires a Ready Node

Server Data
    └─ requires an execution location and filesystem access

Node
    ├─ uses Node Agent Node capability
    └─ uses Akamai Compute Adapter
```

依存関係はDAGであり、すべてのmoduleが平らに相互依存する構造ではありません。

## Process boundary and module boundary

Control Plane側とNode Agent側には対応するdomain moduleがあります。

| Control Plane | Node Agent | Responsibility across boundary |
| --- | --- | --- |
| `node` | `node` | Node observation、bootstrap result、Node-level operation |
| `workload` | `workload` | Workload apply/start/stop/observe |
| `server_data` | `server_data` | backup、restore、check、prune |
| `minecraft` | `minecraft` | Minecraft-specific observe/save/stop/control |

対応は「内部構造を完全に鏡写しにする」という意味ではありません。同じdomain languageでtyped contractを持ち、transport coreやexternal toolのdetailを跨いで漏らさないという意味です。

## Authority

- Control Planeはdesired state、policy、durable operation、Incidentを所有する
- Node Agentはlocal execution、local observation、local adapter selectionを所有する
- Akamai CloudはCompute Instanceのprovider truthを持つ
- restic repositoryはbackup dataとSnapshotのtruthを持つ
- Minecraft processはapplication stateのtruthを持つ

Control Planeのdatabaseはexternal systemの代替truthではありません。外部stateをtimestamp付きObservationとして保持します。

## Initial build order

```text
Foundation
  → Node Management
  → Workload Runtime
  → Server Data
  → Minecraft Server Control
  → Lifecycle Orchestration
```

詳細は[`plans/`](../plans/README.md)を参照してください。
