# Minecraft Server Management System

A self-hosted automation system for running Minecraft Java servers with replaceable cloud nodes and restorable server state.

Minecraft Server Management Systemは、小規模なcommunityがMinecraft serverを**安全に、スマートに、自動運用する**ためのsystemです。OperatorはNode、Podman、restic、RCON、cloud APIを個別に操作せず、Minecraft Serverのdesired stateだけを変更します。

## Current stage

このrepositoryはdocumentation-firstの段階です。source codeはまだありません。

現在のarchitectureは、実装範囲を次へ絞っています。

- Minecraft runtimeは[`itzg/minecraft-server`](https://github.com/itzg/docker-minecraft-server)だけを正式に扱う
- public resourceは`MinecraftServer`、`Node`、`Snapshot`、`Operation`を中心とする
- Control PlaneとNode AgentはJSON-RPC 2.0 over HTTP/2で通信する
- Agentがoutbound connectionとpollを開始する
- command deliveryはat-least-once、effectはidempotentにする
- `/data`とruntime configurationを一つの`Server Home`としてbackup・restoreする
- restic exit code 0とSnapshot ID取得をbackup成功とする

## System at a glance

```text
Operator clients
  ├─ mcserverctl
  └─ Discord Bot / local automation
          │
          │ JSON-RPC 2.0 over HTTP/2 over Unix socket
          ▼
mcserver-control-plane
          │
          ├─ Akamai Cloud API
          │
          │ JSON-RPC 2.0 over HTTPS / HTTP/2
          │ Agent-initiated sync
          ▼
mcserver-node-agent
          ├─ systemd / Podman / Quadlet
          ├─ itzg/minecraft-server
          ├─ local RCON
          └─ restic / Cloudflare R2
```

- **Control Plane**はdesired state、Operation、policy、controller、Node allocationを所有します。
- **Node Agent**はNode上のlocal mechanismをtyped operationとして実行し、observationとresultを返します。
- **Server Home**はMinecraftの`/data`と、そのdataを起動するitzg runtime configurationを一体で保持します。
- **Node**は交換可能です。server identityと永続状態はNodeに依存しません。

## Documentation

入口は[`docs/index.md`](docs/index.md)です。最初は次の順で読むことを推奨します。

1. [`docs/vision.md`](docs/vision.md)
2. [`docs/system-model.md`](docs/system-model.md)
3. [`docs/terminology.md`](docs/terminology.md)
4. [`docs/architecture/overview.md`](docs/architecture/overview.md)
5. [`docs/plans/architecture-reset.md`](docs/plans/architecture-reset.md)
6. [`docs/plans/local-node-v1.md`](docs/plans/local-node-v1.md)

Repositoryを変更する人またはautomationは、最初に[`AGENTS.md`](AGENTS.md)も確認してください。
