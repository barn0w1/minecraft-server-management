# Minecraft Server lifecycle orchestration

## Purpose

`MinecraftServerController`はMinecraftServer Specを、Node allocation、Server Home、Server Runtime、Snapshot、Operationへ展開してdesired lifecycleへ収束させます。

## Start Operation

```text
server.start
  → acquire_node
  → wait_agent
  → prepare_server_home
  → restore_snapshot_if_needed
  → apply_generation
  → start_runtime
  → wait_readiness
  → complete
```

- Server Homeが既にcurrent Nodeにある場合はrestoreを省略する
- 初回起動ではempty Server Homeを作成する
- replacement Nodeでは指定またはlatest successful Snapshotからrestoreする
- each Stageはidempotentであり、restart後に再評価できる

## Stop Operation

```text
server.stop
  → request_graceful_stop
  → wait_runtime_stopped
  → backup_server_home
  → release_node_if_policy
  → complete
```

`OnDemand` policyでNodeをreleaseする場合、stop後のbackup Stage成功を要求します。restic exit code 0とSnapshot ID取得でbackup Stageを成功とします。

Nodeを残すpolicyでは、Operator設定によりbackupを省略できます。

## Backup Operation

### Offline

runtimeがStoppedならServer Homeへ直接restic backupを実行します。

### Online

runtimeがRunningならRCON save quiesce、restic backup、save resumeを一つのOperationとして扱います。

backup successとsave resume failureは別々にstatusへ表現できます。Snapshotが作成されてもsaveが再開できなければ`Degraded=True`です。

## Restore Operation

```text
server.restore
  → require_runtime_stopped
  → inspect_destination
  → prepare_empty_destination
  → restic_restore_snapshot
  → read_manifest
  → reconcile_desired_spec
  → complete
```

unknown fileを持つdestinationへsilent overwriteしません。既存Server Homeを置換する場合はOperatorが明示したrestore modeまたはsystem-owned staging directoryを使用します。

Snapshot manifestとcurrent Specが異なる場合、次を区別します。

- `RestoreAndUseSnapshotConfig`: manifestのconfigurationをnew Specとしてimportする
- `RestoreDataAndApplyCurrentSpec`: dataをrestore後、current Specをmaterializeする

exact CLI/APIはRestore milestoneで定義します。

## Update Operation

```text
server.update
  → optional_pre_update_backup
  → stop_runtime_if_required
  → apply_new_generation
  → start_runtime
  → wait_readiness
```

Minecraft version migrationを伴うupdateのrollbackは、previous container configurationだけでなくpre-update Snapshot restoreを明示的に選択します。

## Node loss recovery

```text
active Node unavailable beyond policy threshold
  → mark RuntimeReady Unknown
  → fence old Allocation
  → provision or select replacement Node
  → restore latest successful Snapshot
  → apply current Spec
  → start runtime
```

Node loss直前のunbacked dataまで復元できるとは主張しません。Operatorへlatest Snapshot timestampを明示します。

## Automation policies

initial policy:

- `AlwaysOn`: Nodeとruntimeを維持する
- `OnDemand`: Stopped後にbackupしてNodeをreleaseする
- `Scheduled`: scheduleがdesired stateを変更する
- `Manual`: Operatorだけがdesired stateを変更する

idle shutdownはplayer observationからControl Planeがdesired stateを`Stopped`へ変更します。itzg image側のindependent auto-stopと二重管理しません。
