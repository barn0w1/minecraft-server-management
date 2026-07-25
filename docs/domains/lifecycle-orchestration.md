# Minecraft Server lifecycle orchestration

## Purpose

`MinecraftServerController`は、Minecraft Server、Server Data、Workload、Nodeを一つのdesired lifecycleとして協調させます。

## Running workflow

```text
MinecraftServer desired state = Running
  → Nodeを要求または選択
  → Node Readyを待つ
  → Server Data restoreを開始
  → verified restore completionを待つ
  → Workload revisionを適用
  → Workload runningを確認
  → Minecraft application readinessを確認
  → MinecraftServer Readyを導出
```

各stepはdurable Operationまたは下位resourceとして追跡し、Control Plane restart後も続行または再評価できるようにします。

## Stopping workflow

```text
MinecraftServer desired state = Stopped
  → 新規接続を必要に応じて抑止
  → Minecraft saveを要求
  → save completionを確認
  → graceful stopを要求
  → Workload stoppedを確認
  → Server Data backupを開始
  → Snapshotとverificationを確認
  → policyに従いNodeをrelease
```

backup前にNodeを削除しません。saveやbackupのoutcomeが不明な場合は、安全側へ停止します。

## Failure isolation

- Minecraft control failureは、必要がなければAkamai mutationを直接blockしない
- backup uncertaintyはNode releaseをblockする
- Node failure時はServer Dataのlast verified Snapshotからreplacement workflowを検討する
- unrelated Minecraft Serverのlifecycleは継続できる

## Orchestration style

generic workflow engineを導入せず、Minecraft Server application layerの明示的なstate machine/controllerとして実装します。共通化は、複数の実workflowで同じpatternが確認された後に行います。
