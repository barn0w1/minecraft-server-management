# Terminology

この文書は、system内で一つのtermが一つの概念を指すためのglossaryです。code、RPC、database、CLI、documentでEnglish termを一貫して使用します。

## Core terms

| Term | Meaning |
| --- | --- |
| Minecraft Server Management System | repositoryが構築するsystem全体 |
| Deployment | 一つのControl Plane、database、credential、managed resource集合 |
| Operator | desired stateを変更し、Operationやstatusを確認するtrusted humanまたはautomation |
| Operator Client | `mcserverctl`、Discord Bot、local automation |
| Control Plane | desired state、Operation、controller、policy、provider accessを所有する中央process |
| Node | Minecraft Serverを実行できるmanaged GNU/Linux machine |
| Node Agent | Node上に常駐し、local operationとobservationを提供するprocess |
| Compute Instance | cloud provider上のVM resource。AkamaiではLinode instance |
| MinecraftServer | Minecraft固有のdesired lifecycleを持つlogical resource |
| Minecraft Server process | Node上で動作するJava server application process |
| Server Home | `data/`、runtime `manifest.json`、server-local secretを含むcomplete restorable directory |
| Data Directory | Server Home内の`data/`。containerの`/data`へmountするdirectory |
| Server Runtime | Server Homeをitzg/minecraft-serverで実行するNode-local mechanism |
| Snapshot | successful restic backupによって作成されたServer Homeの時点copy |
| Repository | 一つのMinecraftServerに対応するrestic repository |
| Deployment Restic Password | Deployment内のすべてのRepositoryで共有するoperator-managed password |

## State and execution terms

| Term | Meaning |
| --- | --- |
| Spec | Operatorまたはpolicyが要求するdesired configuration |
| Generation | Spec変更ごとに増えるmonotonic version |
| Status | ObservationとOperationから導出したcurrent summary |
| Observation | source、timestamp、freshnessを持つ外部またはNode-local fact |
| Condition | `Ready`、`Progressing`、`Degraded`などのcurrent derived state |
| Event | 過去の重要なtransitionを表すbounded record |
| Operation | durable ID、kind、stage、attempt、resultを持つ長時間処理 |
| Operation Stage | Operation内のidempotentな一段階。Agent journalではOperation IDと組み合わせてkeyにする |
| Controller | desired stateとcurrent stateを比較してbounded reconcileを繰り返すcomponent |
| Reconciliation | resourceを読み、安全な次のactionを最大一つ決める一回の処理 |
| Command | Control PlaneがAgentへ返すtyped Operation Stage instruction |
| Agent Sync | Agentがobservation、operation updateを送り、Commandを受け取るJSON-RPC call |
| Agent Session | Agent process起動ごとのsession IDと、そのsync activity |
| Fencing Token | 古いNode allocationやcommandを拒否するmonotonic token |
| Incident | 自動的に安全な選択ができず、Operator判断が必要なexceptional record |
| Ready | resourceが上位operationを受け入れられることを表すCondition |

## Architecture terms

| Term | Meaning |
| --- | --- |
| Operator API | local trusted clientからControl PlaneへのJSON-RPC interface |
| Agent API | Node AgentからControl PlaneへのJSON-RPC interface |
| Adapter | external systemまたはlocal toolをtyped internal contractへ変換するcomponent |
| Store | Control Plane application stateを永続化するcomponent |
| Owned Resource | Deployment metadataとstored identityによりmutation可能と判断したexternal resource |
| Allocation | MinecraftServerとNodeのactive binding |

## Artifact names

| Role | Name |
| --- | --- |
| repository | `minecraft-server-management` |
| Control Plane binary | `mcserver-control-plane` |
| Node Agent binary | `mcserver-node-agent` |
| operator CLI | `mcserverctl` |
| Control Plane unit | `mcserver-control-plane.service` |
| Node Agent unit | `mcserver-node-agent.service` |
| Operator socket | `/run/mcserver/control-plane.sock` |
| configuration root | `/etc/mcserver/` |
| persistent state root | `/var/lib/mcserver/` |
| Server Home root | `/var/lib/mcserver/servers/<server-id>/` |
| Deployment Restic Password file | `/etc/mcserver/secrets/restic-password` |

## Naming rules

- `Host`ではなく`Node`を使用する
- generic `Workload` resourceを使用しない
- `Server Data`をcomplete restorable unitの意味で使用しない。正式名称は`Server Home`
- logical `MinecraftServer`と`Minecraft Server process`を区別する
- `Node ID`と`Compute Instance ID`を区別する
- JSON-RPC request IDとOperation IDを区別する
- retry中の通常failureを`Incident`と呼ばない
