# System model

この文書は、Minecraft Server Management Systemのresource、process、state、end-to-end lifecycleを一つのmental modelとして説明します。

## Primary resources

Operatorから見える主要resourceは四つです。

```text
MinecraftServer
Node
Snapshot
Operation
```

### MinecraftServer

管理対象のlogical Minecraft serverです。desired lifecycle、Minecraft version、server type、itzg configuration、automation policy、active Node、current statusを持ちます。

`MinecraftServer`はlogical resourceです。Node上のJava processだけを指す場合は`Minecraft Server process`と表記します。

### Node

Minecraft Serverを実行できるmanaged GNU/Linux machineです。system上の`Node ID`とcloud provider上の`Compute Instance ID`は別identityです。

Nodeは交換可能です。v1では一つのNodeへ同時に一つのMinecraft Serverだけをallocateします。

### Snapshot

restic repositoryへ成功して保存された`Server Home`の時点copyです。SnapshotはMinecraft Server ID、spec generation、source Node、consistency modeと関連付けます。

### Operation

start、stop、backup、restore、update、Node provision、Node deleteなどの長時間処理を追跡するdurable resourceです。Operationは現在stage、attempt、next retry、resultを持ち、Control Plane restart後も再開できます。

## Internal concepts

### Server Home

一つのMinecraft Serverを復元して起動するためのNode-local directoryです。

```text
/var/lib/mcserver/servers/<server-id>/
├─ manifest.json
├─ secrets/
│  └─ rcon-password
└─ data/
```

- `data/`はitzg/minecraft-serverの`/data`へmountする
- `manifest.json`は適用されたMinecraft Server spec、image reference、environment、port、resource setting、generationを記録する
- `secrets/`はそのMinecraft Serverの起動に必要なserver-local secretを保持する
- Server Home全体をresticのbackup・restore対象にする

これによりSnapshotはworldだけでなく、そのworldをどの設定で起動していたかも保持します。

### Server Runtime

Node AgentがServer Homeからmaterializeする実行環境です。v1では必ず次で構成します。

```text
systemd unit
  → Podman / Quadlet
  → itzg/minecraft-server container
  → Server Home/data mounted at /data
```

Server Runtimeは独立したpublic resourceではなく、MinecraftServerのNode-local実装です。

### Observation, Condition, Event

- `Observation`: Agent、provider、systemd、Podman、RCON、resticから得たtimestamp付き事実
- `Condition`: 現在状態を要約する`Ready`、`Progressing`、`Degraded`などのderived state
- `Event`: `RuntimeStarted`、`BackupSucceeded`などのboundedな履歴

## Running processes

```text
Control Plane host                       Managed Node
┌────────────────────────────┐           ┌────────────────────────────┐
│ mcserver-control-plane     │           │ mcserver-node-agent        │
│                            │◀──────────│                            │
│ SQLite                     │ HTTPS/h2  │ local operation journal    │
│ controllers                │ JSON-RPC  │ Podman/Quadlet/systemd      │
│ durable Operations         │ Agent pull│ itzg/RCON/restic            │
└────────────┬───────────────┘           └────────────────────────────┘
             │
             └─ Akamai API / Cloudflare R2 metadata
```

### Control Plane

Control Planeは中央authorityです。

- OperatorのSpecを永続化する
- resourceをreconcileする
- Operationを作成・進行する
- Agentへ実行commandを割り当てる
- provider APIを呼ぶ
- ConditionとEventを生成する

### Node Agent

Node AgentはNode-local mechanismを所有します。

- Agent APIへoutbound HTTP/2 syncを行う
- commandをlocal journalへ記録する
- same commandを再受信しても同じeffectへ収束させる
- Server Homeを作成・restoreする
- Quadletをmaterializeし、itzg runtimeを起動・停止する
- RCON、systemd、Podmanを観測する
- restic backup・restoreを実行する

Control Planeからarbitrary shell commandを受け取りません。

## Communication model

AgentはControl PlaneへJSON-RPC 2.0 requestを送ります。Control PlaneからNodeへの直接inbound connectionはありません。

```text
Agent ── agent.sync request ──> Control Plane
Agent <─ commands in result ─── Control Plane
```

HTTP/2 connectionを再利用し、long pollによってidle時のrequest頻度を抑えます。Control Planeは同じcommandを再配送でき、Agentは`operation_id`と`stage`をkeyに重複実行を吸収します。

## Desired state and reconciliation

MinecraftServer Spec変更ごとに`generation`が増えます。Controllerはdesired generationとAgentが報告した`applied_generation`を比較します。

```text
desired Spec
  + durable Operation
  + fresh Observation
  → one bounded reconciliation
  → next stage or retry time
```

一回のreconciliationは長時間待ちません。外部処理を開始したらOperationを保存して終了し、後続syncまたはtimerで再評価します。

## Starting a Minecraft Server

```text
1. desired_state = Running
2. active Operationを作成または再開
3. Nodeを選択またはprovision
4. Agent availabilityを待つ
5. Server HomeがなければSnapshotからrestore、初回なら作成
6. desired Specをmanifestへmaterialize
7. Quadlet/systemdへruntimeを適用
8. itzg containerを起動
9. healthcheckとRCON readinessを確認
10. Ready conditionをTrueにする
```

## Stopping and backing up

```text
1. desired_state = Stopped
2. RCONまたはcontainer lifecycleでgraceful stop
3. process停止を観測
4. Server Home全体へrestic backup
5. restic exit code 0とSnapshot IDをresultとして保存
6. policyがOnDemandならNode release
```

running中のmanualまたはscheduled backupでは、RCONでsaveをquiesceし、`save-on`を必ず復帰させてからresultを確定します。

## Replacement recovery

Nodeを失った場合、last successful Snapshotがあれば新しいNodeへServer Homeをrestoreし、同じMinecraftServer Spec generationを適用します。古いNodeが後から復帰しても、allocationごとの`fencing_token`が古いcommandを拒否します。

## Sources of truth

| Information | Source of truth |
| --- | --- |
| desired Minecraft configuration | Control Plane database |
| active Operationとstage | Control Plane database |
| local command result | Agent operation journal |
| Compute Instance existence | Akamai API |
| local runtime state | systemd、Podman、RCON observation |
| backup resultとSnapshot ID | successful restic command result |
| Snapshot data | restic repository on R2 |

Control Plane databaseは外部systemの状態を推測して置き換えません。ただし、resticやR2の内部整合性を独自に再検証することもしません。
