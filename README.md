# Minecraft Server Management System

A self-hosted system for managing Minecraft server lifecycles, infrastructure, workloads, and data.

このrepositoryは、小規模なcommunityがMinecraft serverを便利に、安全に、堅固に運用するためのmanagement systemを新しく構築するためのものです。企業向けSaaSやmulti-tenant hosting platformを目標にはしませんが、data保全、identity、ownership、restart recovery、destructive operationの安全性は規模に関係なく重視します。

現在は**design-first foundation**の段階です。実装はまだ置かず、過去のprototypeと設計経験から有効な知識だけを抽出し、現在の正本となるdocumentationを整備しています。

## Components

```text
Operator clients
  ├─ mcserverctl
  └─ local automation / Discord bot
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
          ├─ Server Data operations
          ├─ Workload Runtime
          └─ Node observation and operations
```

- **Control Plane**: desired state、durable state、policy、controller、orchestrationを所有する中央process
- **Node Agent**: 管理対象Node上でlocal operationとobservationを提供する常駐process
- **`mcserverctl`**: Control PlaneのOperator APIを利用するfull-control CLI

## Documentation

設計の入口は[`docs/index.md`](docs/index.md)です。

特に最初に読む文書:

1. [`docs/vision.md`](docs/vision.md)
2. [`docs/terminology.md`](docs/terminology.md)
3. [`docs/design-lineage.md`](docs/design-lineage.md)
4. [`docs/architecture/overview.md`](docs/architecture/overview.md)
5. [`docs/design-principles.md`](docs/design-principles.md)
6. [`docs/plans/foundation.md`](docs/plans/foundation.md)

AI agentとrepository作業者は、変更前に[`AGENTS.md`](AGENTS.md)を確認してください。
