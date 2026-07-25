# Terminology

この文書は、system内で一つの言葉が一つの概念を指すようにするためのglossaryです。English termをcode、RPC、database、CLI、documentで一貫して使用します。日本語本文でも、定義済みのtermは原則としてEnglish表記を維持します。

| Term | Meaning |
| --- | --- |
| Minecraft Server Management System | repositoryが構築するsystem全体 |
| Operator | systemのdesired stateを変更し、Incidentやoperationを確認するtrusted humanまたはautomation |
| Operator Client | Operator APIを利用する`mcserverctl`、Discord Bot、local automationなどのprocess |
| Operator API | trusted local clientがControl Planeを操作するJSON-RPC interface |
| Control Plane | desired state、durable state、policy、controller、orchestrationを所有する中央process |
| Node | systemに登録され、Workloadを実行する管理対象GNU/Linux machine |
| Node Agent | Node上に常駐し、local operationとobservationを提供するprocess |
| Agent Protocol | Control PlaneとNode Agentの間のauthenticated cross-machine protocol |
| Agent Session | 一つのauthenticated Node Agent connectionと、そのconnectionに属するliveness state |
| Compute Instance | cloud providerが所有するVM resource。AkamaiではLinode instance |
| Minecraft Server | Minecraft固有のdesired lifecycleを持つlogical resource |
| Minecraft Server process | Node上で実際に動作するMinecraft Java server application process |
| Server Data | Minecraft Serverに属する永続file treeと、そのbackup/restore lifecycle |
| Workload | Node上で実行されるprogram、container、mount、port、environmentなどのdesired execution unit |
| Workload Runtime | Node Agent内でWorkloadをPodman、Quadlet、systemdへ適用するmodule |
| Controller | desired stateとobserved stateを比較し、boundedな一回のreconciliationを繰り返すcomponent |
| Reconciliation | durable stateとexternal observationを評価し、安全な次のactionを決める一回の処理 |
| Spec | userまたは上位domainが要求したdesired state |
| Status | observationとdurable stateから導出されるcurrent state |
| Observation | timestamp、source、freshnessを持つexternal systemから得た事実 |
| Operation | durable IDとlifecycleを持ち、結果を後から確認できる処理 |
| Mutation | external systemまたはNode上のstateを変更するoperation |
| Incident | 自動的に安全な進行を続けられず、人間の認識または介入が必要なdurable record |
| Ready | resourceが上位domainの要求を受け入れられることを示すderived condition。単一provider statusではない |
| Capability | Node Agentが現在のNode上で安全に提供できるtyped operationまたはobservation |
| Adapter | external systemを内部portへ変換するinfrastructure component |
| Client | external network protocolを呼び出すcomponent |
| Store | application stateを永続化するcomponent。restic Repositoryとの混同を避ける |
| Repository | Server Dataのbackupを保持するrestic repository |
| Snapshot | restic repository内の特定時点のfile tree |
| Owned Resource | Deployment identityとprovider metadataによって、このsystemがmutationしてよいと証明されたexternal resource |
| Deployment | 一つのControl Plane、trust domain、database、managed resource集合 |

## Artifact names

| Role | Name |
| --- | --- |
| repository | `minecraft-server-management` |
| Control Plane binary | `mcserver-control-plane` |
| Node Agent binary | `mcserver-node-agent` |
| operator CLI | `mcserverctl` |
| Control Plane systemd unit | `mcserver-control-plane.service` |
| Node Agent systemd unit | `mcserver-node-agent.service` |
| local Operator socket | `/run/mcserver/control-plane.sock` |
| configuration root | `/etc/mcserver/` |
| persistent state root | `/var/lib/mcserver/` |

## Naming rules

- `Host`ではなく`Node`を使用する
- `Manager`より具体的な`Controller`、`Runtime`、`Client`、`Adapter`、`Store`を使用する
- provider resourceとlogical resourceを区別する。`Node ID`と`Compute Instance ID`は別identity
- logical `Minecraft Server`と実際の`Minecraft Server process`を区別する
- `Control Plane`はarchitecture上の役割名、`mcserver-control-plane`はOS上のartifact名
- Rust moduleは`node`、`workload`、`server_data`、`minecraft`のようにdomain名を使う
- JSON-RPC methodはlowercase namespaceを使用する。例: `node.observe`、`workload.apply`
