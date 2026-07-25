# Scope

## v1 system scope

このsystemは次を管理します。

- `MinecraftServer`のdesired state、configuration、status
- itzg/minecraft-serverをPodman、Quadlet、systemdで実行するruntime
- `/data`とitzg runtime configurationを一体にした`Server Home`
- RCONによるreadiness、player observation、save、graceful stop
- resticによるCloudflare R2へのbackupとrestore
- Akamai Cloud上のCompute Instance provisioningと削除
- GNU/Linux Nodeのbootstrap、registration、observation、allocation
- JSON-RPC 2.0 over HTTP/2によるOperator APIとAgent API
- durable `Operation`、retry、restart recovery、status condition、event
- `mcserverctl`、Discord Bot、local automationなどのtrusted client

## Initial deployment profile

- Control PlaneはOCI Compute上の一つのprocess
- Control Plane databaseはlocal SQLite
- managed NodeはAkamai Cloud Compute Instanceまたは手動登録Node
- Node OSはDebian GNU/Linuxをinitial targetとする
- Node AgentはControl Planeへoutbound HTTPS connectionを作る
- Agent APIはHTTP/2を要求する
- Minecraft runtimeはitzg/minecraft-serverのみ
- 一つのMinecraft Serverに同時に一つのactive Node
- 一つのNodeに同時に一つのMinecraft Server
- backup backendはCloudflare R2上のrestic repository
- 一つのMinecraft Serverにつき一つのrestic repository
- すべてのrepositoryで一つのDeployment Restic Passwordを共有する

## Explicit non-goals

v1では次を実装しません。

- public hosting SaaS、multi-tenancy、billing
- general-purpose container orchestrator
- Minecraft以外のworkload
- generic Workload resource
- generic cloud provider plugin ecosystem
- generic workflow engine
- message broker、distributed database、consensus protocol
- Control Plane high availability
- Node pool、bin packing、live migration
- public remote Operator API
- arbitrary remote shellまたはarbitrary RCON proxy
- private PKI、offline Root CA、short-lived Agent certificate rotation
- backupごとの追加repository verification
- 定期`restic check`をsystem invariantにすること
- 自動restore drillをv1 completion条件にすること
- pre-stable schema、RPC、CLIとのbackward compatibility

## Trust assumptions

- restic exit code 0は、全source fileを含むSnapshotが作成された成功結果として扱う
- Cloudflare R2のdocumented consistencyとdurabilityをstorage contractとして信頼する
- systemd、Podman、itzg/minecraft-serverのdocumented behaviorを再実装しない
- managed Nodeのrootはtrusted boundaryとする
- Operator APIへaccessできるlocal processはfull-control clientとする

## Milestone discipline

全systemをhorizontal layerごとに作りません。まず手動登録Node上でMinecraft Serverを起動・停止できるvertical sliceを完成させ、その後durability、backup、Akamai automationを追加します。詳細は[`plans/`](plans/README.md)を参照してください。
