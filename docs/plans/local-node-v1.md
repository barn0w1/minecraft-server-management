# Local Node v1 plan

Status: Proposed

## Goal

OperatorがControl PlaneへMinecraftServer Specを登録し、手動登録したreal GNU/Linux Node上でitzg/minecraft-serverをstart、observe、stopできる最初のend-to-end vertical sliceを完成させます。

```text
mcserverctl
  → Operator API
  → Control Plane
  ← Agent Sync
  → Node Agent
  → Quadlet / Podman / systemd
  → itzg/minecraft-server
  → local RCON readiness
```

## In scope

- Rust workspaceとthree binaries: Control Plane、Node Agent、CLI
- SQLite migration foundation
- local Operator API: JSON-RPC over HTTP/2 over Unix socket
- Agent enrollment: one-time tokenとper-Node credential
- Agent API: JSON-RPC over HTTPS/HTTP/2
- `agent.sync` request/result
- manual Node registration
- minimal durable Operation
- local Agent operation journal
- MinecraftServer create/get/list
- explicit `TYPE`、`VERSION`、image、memory、port Spec
- Server Home directoryとmanifest materialization
- RCON password file
- Quadlet generation
- start、readiness、status、graceful stop
- structured log、Condition、Event

## Out of scope

- Akamai provisioning/delete
- Cloudflare R2、restic backup/restore
- automatic Node replacement
- scheduled start/stop
- idle shutdown
- Discord Bot
- retention
- mTLS/private PKI
- generic Workload support

## Required decisions before code

- Rust HTTP/2 server/client library
- JSON schema/serde policyとprotocol version field
- local development TLS certificate approach
- canonical ID formats
- SQLite migration tool
- Quadlet rootfulまたはrootless execution model
- Server Home ownership UID/GID
- initial itzg image reference policy
- initial supported `TYPE` subset
- readiness deadlineとsync long-poll default

これらはarchitectureを変更しないimplementation decisionです。plan内またはsmall ADRで確定します。

## Implementation slices

### Slice 1: workspace and process skeleton

- `control-plane`
- `node-agent`
- `cli`
- `protocol`
- configuration、logging、shutdown

### Slice 2: Operator API vertical slice

- Unix socket lifecycle
- HTTP/2 prior knowledge
- JSON-RPC codec
- `system.version`
- typed CLI client

### Slice 3: Control Plane persistence

- SQLite migration
- MinecraftServer Spec/Generation
- Node registration
- Operation、Condition、Event minimum schema
- restart test

### Slice 4: Agent enrollment and sync

- one-time token
- Agent Credential
- HTTPS/HTTP/2 endpoint
- `agent.enroll`
- `agent.sync`
- liveness and capability report

### Slice 5: Agent operation journal

- `operation_id + stage` key
- payload hash
- Accepted/Running/Succeeded/Failed
- duplicate Command replay
- restart recovery

### Slice 6: Server Home and runtime

- directory creation
- manifest schema
- RCON password file
- Quadlet materialization
- systemd reload/start/stop
- itzg container `/data` mount

### Slice 7: Minecraft control and status

- RCON observe
- readiness
- player count if available
- graceful stop
- Condition/Event mapping
- human-readable `mcserverctl server status`

## Acceptance criteria

### Operator flow

- `mcserverctl server create`でSpecを保存できる
- `mcserverctl server start`がOperation IDを返す
- `mcserverctl server status`でstage、Node、Agent freshness、runtime、readinessを確認できる
- `mcserverctl server stop`でRCON graceful stopとprocess停止を確認できる

### Protocol

- Agent APIはHTTP/2でconnection reuseする
- Agentがoutbound requestだけでCommandを受け取れる
- malformed JSON-RPCとunsupported methodをstable errorへmappingする
- same Commandを複数回返してもruntime apply effectが一つへ収束する
- Agent restart後にjournal resultを再報告できる

### Runtime

- Server Home `data/`がcontainer `/data`へmountされる
- effective Specがmanifestへ書かれる
- `TYPE`と`VERSION`がexplicitである
- RCONはpublic portへpublishされない
- systemd、container、health、RCONのANDでReadyを導出する

### Recovery

- Control Plane restart後にactive Operationを再読込できる
- Agent disconnect中はOperationがWaitingになり、reconnect後に自動再開する
- stale generationのCommandをAgentが拒否する

## Exit condition

Akamaiとbackupを使わなくても、real Node上でMinecraft Server lifecycleのcore loopを実証し、後続milestoneが同じOperation、Agent API、Server Home、runtime contractを再利用できること。
