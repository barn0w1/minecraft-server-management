# Architecture overview

この文書はsystem全体のprocess boundary、authority、dependencyを要約します。resourceとlifecycleの導入は[`system-model.md`](../system-model.md)を参照してください。

## System shape

```text
Operator clients
    │ JSON-RPC / HTTP/2 / Unix socket
    ▼
Control Plane
    ├─ MinecraftServer Controller
    ├─ Node Controller ───────────────> Akamai Cloud
    ├─ Operation Engine
    ├─ Snapshot metadata
    └─ SQLite
    ▲
    │ JSON-RPC / HTTPS / HTTP/2
    │ Agent-initiated sync
    │
Node Agent
    ├─ Server Runtime ────────────────> systemd / Podman / Quadlet
    │                                     └─ itzg/minecraft-server
    ├─ Minecraft control ─────────────> local RCON
    ├─ Server Home ───────────────────> filesystem
    └─ Backup adapter ────────────────> restic / Cloudflare R2
```

## Primary aggregate

`MinecraftServer`がprimary aggregateです。Node、Server Home、Snapshot、OperationはMinecraft Server lifecycleを支えます。

```text
MinecraftServer
  ├─ desired Spec and Generation
  ├─ active Node Allocation
  ├─ Server Home
  ├─ latest Snapshot
  ├─ current Operation
  └─ Conditions
```

Server Runtimeはpublic resourceではなく、Node AgentがMinecraftServer Specをmaterializeしたlocal implementationです。

## Authority

| Concern | Authority |
| --- | --- |
| desired Minecraft configuration | Control Plane database |
| Operation stage、retry、result | Control Plane database |
| Agent command execution result | Agent local operation journal |
| Compute Instance existence | Akamai API |
| active Node allocation | Control Plane database + Fencing Token |
| runtime state | systemd、Podman、RCON observation |
| Server Home contents | Node filesystem |
| backup success and Snapshot ID | restic command result |
| Snapshot bytes | restic repository on R2 |

## Dependency direction

```text
MinecraftServer application
    ├─ uses Node allocation service
    ├─ creates durable Operations
    └─ dispatches typed Agent Commands

Node application
    └─ uses Akamai adapter and Agent observation

Agent capabilities
    ├─ runtime
    ├─ minecraft
    ├─ server_home
    └─ backup
```

Akamai、Podman、restic、RCONのconcrete typeやcommand lineをapplication domainへ漏らしません。

## Control loop

Controllerは一回のreconciliationで次を行います。

```text
load Spec, Status, active Operation
  → evaluate fresh Observation
  → validate allocation and generation
  → persist one next stage or retry time
  → return
```

外部処理はOperation StageとしてAgentまたはprovider adapterへ委譲します。eventはreconcileを早めますが、正しさはdatabaseとobservationに依存します。

## Build order

```text
Architecture Reset
  → Local Node vertical slice
  → Durable Operations and reconnect
  → Backup and restore
  → Akamai Node lifecycle
  → Smart automation
  → Hardening
```

詳細は[`plans/`](../plans/README.md)を参照してください。
