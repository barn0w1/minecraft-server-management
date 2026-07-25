# System model

この文書は、Minecraft Server Management Systemを初めて読む人向けに、管理対象、主要resource、process、dependency、end-to-end lifecycleを一つのmental modelとして説明します。

## What the system manages

Operatorが管理したい対象は、単なるCloud VMや一つのJava processではありません。

```text
Minecraft Server
  ├─ Minecraft固有のdesired stateとoperation
  ├─ Server Data
  ├─ Workload
  └─ execution先となるNode
```

Systemはこれらを別resourceとして扱い、最上位のMinecraft Server lifecycleで協調させます。

### Minecraft Server

Minecraft固有のlogical resourceです。Minecraft version、server distribution、configuration、application readiness、save、graceful stop、playerやserver stateなどを扱います。

`Minecraft Server`はlogical resourceの名称です。Node上で実際に動くJava processだけを指す場合は`Minecraft Server process`と表記します。

### Server Data

Minecraft Serverに属する永続file treeです。world、player data、plugins、mods、configuration、server software固有dataなどを含みます。

Server Data domainはfileの内容をMinecraftの意味として解釈せず、opaqueなbytes、path、metadata、Snapshotとして管理します。Minecraft固有のsave consistencyはMinecraft Server domainが作ります。

### Workload

Node上でprogramを安全に実行するためのdesired execution unitです。container image、process、environment、mount、port、resource limit、systemd unitなどを扱います。

Minecraft ServerはWorkloadとして実行されますが、Workload domainはMinecraft commandやplayerの意味を知りません。

### Node

Workloadを実行できるmanaged GNU/Linux machineです。Nodeはsystem上のlogical resourceであり、Akamai Cloud上の`Compute Instance`とは別identityです。

```text
Node ID                  systemのlogical identity
Compute Instance ID      cloud provider上のresource identity
```

Nodeは交換可能です。Compute Instanceが失われても、Server Dataをverified Snapshotから新しいNodeへrestoreできることを目指します。

## Running processes

System自身の主要processは二つです。

```text
Control Plane VM                         Managed Node
┌──────────────────────────┐             ┌──────────────────────────┐
│ mcserver-control-plane   │   QUIC      │ mcserver-node-agent      │
│                          │◀═══════════▶│                          │
│ desired state            │   mTLS      │ local execution          │
│ durable operations       │  JSON-RPC   │ local observation        │
│ controllers              │             │ local adapters           │
└──────────────────────────┘             └──────────────────────────┘
```

### Control Plane

Control Planeは中央のauthorityです。

- Operatorが要求したdesired stateを永続化する
- controllerを実行する
- domain間のoperation順序を決める
- durable OperationとIncidentを所有する
- external stateをObservationとして保存する
- Node Agentへtyped operationを送る

### Node Agent

Node Agentは各managed Nodeに常駐するnode-local execution planeです。

- GNU/Linuxとsystemdを観測する
- Podman、Quadlet、systemdを通してWorkloadを操作する
- resticとfilesystemを通してServer Data operationを実行する
- Minecraft Server Management ProtocolやRCONを通してMinecraft Server processを操作する
- Control Planeへheartbeat、report、operation resultを送る

Node Agentは多くのlocal機能を持ちますが、一つの巨大なmoduleではありません。`node`、`workload`、`server_data`、`minecraft`のcapabilityを分離したmodular monolithです。

### Operator clients

`mcserverctl`、Discord Bot、local automationはControl PlaneのOperator API clientです。

```text
mcserverctl ───────┐
Discord Bot ───────┼─ JSON-RPC over Unix domain socket ─▶ Control Plane
local automation ──┘
```

最初は同じUnix socketと同じControl Plane権限を使用します。Discord userごとのauthorizationはBot側で行い、Bot process自体はtrusted full-control clientとして扱います。

## Domain dependency

Control PlaneとNode Agentは同じdomain languageを共有しますが、内部構造を完全に鏡写しにはしません。

```text
Minecraft Server lifecycle
    ├─ uses Minecraft Server operations
    ├─ uses Server Data operations
    ├─ uses Workload operations
    └─ uses Node lifecycle

Workload
    └─ requires a Ready Node

Server Data
    └─ requires a Node-side execution location and filesystem access

Node
    ├─ uses Node Agent Node capability
    └─ uses Akamai Compute Adapter
```

Dependencyは一方向です。

- NodeはMinecraftを知りません。
- WorkloadはMinecraftのsave semanticsを知りません。
- Server DataはMinecraft file contentsを解釈しません。
- Minecraft ServerはAkamai API、Podman command、restic commandを直接呼びません。

## Example: starting a Minecraft Server

OperatorがMinecraft Serverを`Running`にすると、Control Planeは一回の巨大transactionではなく、durable stateとobservationを使って段階的に収束させます。

```text
1. MinecraftServer desired state = Running
2. 実行可能なNodeを確保する
3. Node Agentがauthenticatedでfreshであることを確認する
4. Server Dataを指定Snapshotからrestoreする
5. Workload definitionをNodeへ適用する
6. Minecraft Server processを起動する
7. Minecraft application readinessを確認する
8. MinecraftServer StatusをReadyへ導出する
```

各stepはrestart後に再評価できます。途中のrequest responseが失われても、外部stateを観測して何が起きたかを確定します。

## Example: stopping and protecting data

```text
1. MinecraftServer desired state = Stopped
2. 必要に応じて新規接続を抑止する
3. Minecraft固有のsaveを要求する
4. save completionを確認する
5. graceful stopを要求する
6. process停止を観測する
7. Server Data backupを実行する
8. Snapshotと必要なverificationを確認する
9. policyが許す場合にNodeをreleaseする
```

`save requestを送った`、`restic commandが終了した`、`Cloud APIがdeleteを受理した`という単一responseだけで次の破壊的stepへ進みません。

## State and sources of truth

| State | Meaning | Owner/source |
| --- | --- | --- |
| Desired state | Operatorまたは上位domainが要求した状態 | Control Plane database |
| Durable application state | Operation、Incident、mutation intentなど | Control Plane database |
| Observation | external systemから取得したtimestamp付き事実 | Akamai、Node Agent、systemd、restic、Minecraft process |
| Status | desired state、durable state、fresh Observationから導出した現在状態 | Control Plane controller |

Control Plane databaseはexternal systemの代替truthではありません。たとえばCompute Instanceの存在はAkamai inventory、Snapshotの存在はrestic repository、Minecraft readinessはMinecraft processの観測で確認します。

## Failure and recovery model

Systemはnetwork、process、providerが失敗することを通常条件として設計します。

- Controllerはlevel-triggeredであり、eventを失ってもstateを再観測できます。
- Control Plane restart後はdatabaseからoperationを再開し、memory上のAgent sessionを有効とはみなしません。
- Node Agentはconnection loss後にjitter付きexponential backoffで再接続します。
- mutation response timeoutは「失敗」ではなく「結果不明」として扱います。
- identity、ownership、external stateが矛盾する場合はIncidentを作り、影響範囲のmutationを停止します。
- provider上のAbsentを確認するまでNode resourceをfinalizeしません。

詳細は[State and reconciliation](architecture/state-and-reconciliation.md)と[Failure model](architecture/failure-model.md)を参照してください。

## Initial deployment profile

最初のdeploymentでは次を使用します。

- Control Plane: OCI Compute上の一つの`mcserver-control-plane`
- managed Node: Akamai Cloud Compute Instance
- Node OS: Debian 13 GNU/Linuxをinitial targetとする
- Workload Runtime: Podman、Quadlet、systemd
- Server Data: restic repository on Cloudflare R2
- Agent communication: QUIC、TLS 1.3、mTLS、JSON-RPC
- Operator communication: JSON-RPC over Unix domain socket

これはinitial implementation profileであり、generic multi-cloud platformやgeneral-purpose orchestratorを約束するものではありません。
