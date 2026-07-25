# Domain overview

主要resourceの関係は[System model](../system-model.md)を参照してください。

v1のdomainは三つのbusiness concernへ整理します。

```text
Minecraft Server
  ├─ uses Server Home
  └─ uses Node

Operationはdomainを横断するdurable execution model
SnapshotはServer Home backupのresult
```

| Domain | Main question |
| --- | --- |
| [Minecraft Server](minecraft-server.md) | どのMinecraft serverを、どの設定で、どの状態にするか |
| [Server Home](server-home.md) | `/data`と起動設定をどのように一体で保存・復元するか |
| [Node](node.md) | 実行可能なGNU/Linux machineをどう確保・allocate・解放するか |
| [Lifecycle orchestration](lifecycle-orchestration.md) | start、stop、backup、restoreをどの順序で進めるか |

`Workload`はv1のdomainではありません。itzg/minecraft-server以外を実行しないため、container runtimeはMinecraft Server domainのinternal `Server Runtime`として扱います。
