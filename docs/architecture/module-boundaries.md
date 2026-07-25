# Module boundaries

## Control Plane modules

```text
interfaces
  ├─ operator_api
  └─ agent_api
        ↓
application
  ├─ minecraft_server
  ├─ node
  ├─ operation
  └─ snapshot
        ↓
domain models and narrow ports
        ↑
infrastructure
  ├─ sqlite
  ├─ http2_jsonrpc
  ├─ akamai
  └─ filesystem/secrets
```

### `minecraft_server`

Spec、Generation、Condition、lifecycle policy、start/stop/update/backup/restore orchestrationを所有します。

### `node`

Node identity、provider binding、allocation、Fencing Token、readiness、provision/deleteを所有します。

### `operation`

Operation lifecycle、stage、attempt、deadline、retry scheduling、eventを所有します。generic workflow languageは持たず、operation kindごとのexplicit controllerを支えます。

### `snapshot`

successful restic resultから得たSnapshot metadataとretention intentを所有します。repository integrity checkerは所有しません。

## Node Agent modules

```text
agent_core
  ├─ enrollment
  ├─ credential
  ├─ http2_sync
  ├─ operation_journal
  └─ task_supervision

capabilities
  ├─ runtime
  ├─ minecraft
  ├─ server_home
  ├─ backup
  └─ node_observation

adapters
  ├─ systemd
  ├─ podman_quadlet
  ├─ itzg
  ├─ rcon
  ├─ filesystem
  └─ restic
```

`agent_core`はMinecraft configurationやbackup policyを決めません。Commandを認証済みNode contextとともにcapabilityへdispatchし、journalへresultを保存します。

## Ownership rules

- moduleは自身が所有するtableだけを直接更新する
- sibling moduleのprivate modelへ依存しない
- RPC handlerはtransactionとlifecycle policyを直接組み立てない
- external adapter typeをdomain public APIへ漏らさない
- wire DTOとdomain modelを同一typeにしない
- shared crateにはJSON-RPC DTO、ID、stable error kindだけを置く

## Allowed dependencies

```text
minecraft_server application → operation, node, agent command ports
node application             → operation, akamai, agent query ports
snapshot application         → operation result and metadata store
agent runtime capability     → systemd, podman, itzg adapters
agent backup capability      → filesystem, restic adapters
agent minecraft capability   → rcon, runtime observation
```

## Forbidden dependencies

- MinecraftServer controllerがAkamai HTTP requestを直接作る
- Control Planeがshell、Podman、restic、RCON command stringを送る
- Node Agentがidle shutdownやNode delete policyを決める
- Snapshot moduleがMinecraft save commandを実行する
- Discord Botがdatabase、Akamai、Agentへ直接接続する
- generic Workload abstractionをMinecraft runtimeの前に挟む
