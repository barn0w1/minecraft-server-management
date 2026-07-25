# Minecraft Server Management System

A self-hosted system for managing Minecraft server lifecycles, infrastructure, workloads, and data.

Minecraft Server Management Systemは、小規模なcommunityが自分たちのMinecraft serverを便利に、安全に、堅固に運用するためのsystemです。Cloud上のGNU/Linux machineを確保し、Minecraft Serverを実行し、永続dataをbackup・restoreし、起動から停止までのlifecycleを一つのdesired stateとして管理します。

## What this repository contains

このrepositoryは、system全体のarchitecture、domain boundary、process間contract、security model、implementation planを定義します。現在は**documentation-first foundation**の段階であり、source codeはまだありません。

最初の実装対象は、Akamai Cloud上にCompute Instanceを作成し、Node Agentをenrollさせ、安全に利用・削除できる`Node Management v1`です。

## System at a glance

```text
Operator clients
  ├─ mcserverctl
  └─ local automation / Discord Bot
          │
          │ JSON-RPC over Unix domain socket
          ▼
mcserver-control-plane
          │
          │ QUIC / TLS 1.3 / mTLS / JSON-RPC
          ▼
mcserver-node-agent
          │
          ├─ Minecraft Server control
          ├─ Server Data backup and restore
          ├─ Workload Runtime
          └─ Node observation and operations
```

- **Control Plane**はdesired state、durable state、policy、controller、orchestrationを所有します。
- **Node Agent**は各managed Nodeに常駐し、local operationとobservationを提供します。
- **`mcserverctl`**はControl PlaneのOperator APIを利用するfull-control CLIです。
- **Discord Botやlocal automation**も、最初はCLIと同じlocal Operator APIを使用するtrusted clientです。

Systemのresource、process、lifecycleを初めて読む場合は、[`docs/system-model.md`](docs/system-model.md)から読むと全体像をつかめます。

## Documentation

Documentationの入口は[`docs/index.md`](docs/index.md)です。

推奨する最初の読み順:

1. [`docs/vision.md`](docs/vision.md)
2. [`docs/system-model.md`](docs/system-model.md)
3. [`docs/scope.md`](docs/scope.md)
4. [`docs/terminology.md`](docs/terminology.md)
5. [`docs/architecture/overview.md`](docs/architecture/overview.md)
6. [`docs/plans/foundation.md`](docs/plans/foundation.md)

Repositoryを変更する人またはAI agentは、最初に[`AGENTS.md`](AGENTS.md)も確認してください。
