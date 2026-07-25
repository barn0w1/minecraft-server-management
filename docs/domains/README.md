# Domain overview

主要domainは次の四つです。

```text
Minecraft Server
  ├─ coordinates Server Data
  ├─ coordinates Workload
  └─ coordinates Node

Server Data
Workload
Node
```

それぞれはconcept、invariant、state ownershipを持ちますが、別microserviceではありません。Control PlaneとNode Agentのmodular monolith内に対応moduleを持ちます。

| Domain | Main question |
| --- | --- |
| [Minecraft Server](minecraft-server.md) | Minecraft applicationをどの状態にしたいか |
| [Server Data](server-data.md) | 永続file treeをどのSnapshotとして保護・復元するか |
| [Workload](workload.md) | Node上でprogramをどう安全に実行するか |
| [Node](node.md) | 使用可能なGNU/Linux execution environmentをどう確保・観測・解放するか |
| [Lifecycle orchestration](lifecycle-orchestration.md) | 複数domainをどの順序で協調させるか |

各domain documentは責務だけでなく、**non-responsibilities**を明示します。
