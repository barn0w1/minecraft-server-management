# Agent API

## Purpose

Node AgentがControl PlaneへauthenticatedなObservationとOperation updateを送り、実行すべきtyped Commandを受け取ります。

## Layering

```text
Agent methods and DTO
  → JSON-RPC 2.0
  → HTTP request/response
  → HTTP/2
  → TLS
  → TCP 443
```

独自length framing、raw QUIC、HTTP/3、WebSocket、server pushはv1で使用しません。

## Direction

すべてのnetwork requestはNode Agentが開始します。

```text
Node Agent ── JSON-RPC request ──> Control Plane
Node Agent <─ JSON-RPC result ──── Control Plane
```

Control PlaneからNodeへinbound connectionを作りません。CommandはAgent Sync resultに含めます。

## HTTP contract

- endpointはstable DNS name上のHTTPS
- application endpointは`POST /agent/v1/rpc`
- HTTP/2をALPN `h2`で要求する
- connectionを再利用する
- `content-type: application/json`
- request bodyは一つのJSON-RPC object
- JSON-RPC batchは使用しない
- HTTP statusはtransport/authentication levelへ使い、domain errorはJSON-RPC errorへ返す
- body、header、deadline、concurrent streamにfinite limitを持つ

## Authentication

### Enrollment

`agent.enroll`だけはone-time Enrollment Tokenを使用します。server TLS identityは必須です。

### Enrolled Agent

通常methodは次を要求します。

```text
Authorization: Bearer <agent-credential>
```

Control Planeはcredential digest、Node ID、active authorizationを検証します。Node IDはrequest paramsだけで信頼せず、credential bindingと一致させます。

## JSON-RPC profile

- `jsonrpc`は`"2.0"`
- request IDはcanonical lowercase UUID string
- `params`はobject
- methodはlowercase namespaced string
- unknown fieldはschema validation error
- batchとnotificationはv1で使用しない
- `result`と`error`は排他的
- stable application errorは`error.data.kind`を持つ
- JSON-RPC IDはidempotency keyではない

## Methods

### `agent.enroll`

one-time tokenをAgent Credentialへ交換します。詳細は[Agent enrollment](agent-enrollment.md)を参照してください。

### `agent.sync`

Agentの通常loopで使用する中心methodです。

request concept:

```json
{
  "jsonrpc": "2.0",
  "id": "<uuid>",
  "method": "agent.sync",
  "params": {
    "node_id": "node-01",
    "session_id": "<uuid>",
    "sequence": 42,
    "agent_version": "...",
    "capabilities": {},
    "observations": [],
    "operation_updates": []
  }
}
```

result concept:

```json
{
  "jsonrpc": "2.0",
  "id": "<same-uuid>",
  "result": {
    "accepted_sequence": 42,
    "server_time": "...",
    "commands": [],
    "next_sync_after_ms": 1000
  }
}
```

exact schemaはprotocol crateでversioned DTOとして定義します。

## Long polling

Commandがない場合、Control Planeは`agent.sync`をinitial default 20秒までholdできます。Agentはdeadlineを少し長く設定します。

HTTP/2 connection上では、operation completionなどurgentなupdateを別streamの`agent.sync`で送信できます。ただし同一Sessionの`sequence`で重複と順序を処理します。

long poll timeoutは正常resultでありerrorにしません。

## Command schema

CommandはControl PlaneがAgentへ割り当てるOperation Stageです。

```json
{
  "operation_id": "<uuid>",
  "stage": "apply_generation",
  "kind": "server.runtime.apply",
  "minecraft_server_id": "survival",
  "spec_generation": 12,
  "fencing_token": 19,
  "deadline": "...",
  "params": {}
}
```

Command kindはtyped allowlistです。shell command、Podman argument list、restic argument list、raw RCON commandを含めません。

## Delivery and idempotency

- Control Planeはterminal updateを受け取るまで同じCommandを再配送できる
- Agent journal keyは`operation_id + stage`
- journalにはpayload hash、state、started_at、completed_at、resultを保存する
- same keyとsame hashなら既存state/resultを返す
- same keyとdifferent hashなら`command_conflict`
- old Fencing Tokenなら`stale_allocation`
- response lossは未実行を意味しない

## Operation updates

Agentは次を報告します。

```text
Accepted
Running
Succeeded
Failed
Rejected
```

terminal resultはAgent journalへ先に保存してからsyncします。Control Planeが受信をacknowledgeするまで、Agentは再送できます。

## Observation

Agent Syncは必要なcurrent factを送ります。

- boot ID、Agent uptime、OS
- Server Home existenceとmanifest generation
- systemd unit state
- container state、image digest、health
- RCON readiness、player count
- active local Fencing Token

heartbeat専用notificationを別protocolとして持たず、fresh Agent Sync受信時刻をlivenessに使用します。

## Session and reconnect

Agent process起動ごとにnew Session IDを作ります。同じNodeのnew Sessionを受理した場合、old Sessionから遅れて届いたsequenceをcurrent stateへ反映しません。

reconnect initial schedule:

```text
1s, 2s, 4s, 8s, 16s, 30s, 60s, 60s ...
```

jitterを加え、successful authenticated sync後にbackoffをresetします。authentication rejectionはnetwork failureと同じ高速retryを行いません。

## Versioning

HTTP pathのmajor versionとDTO schema versionでincompatible changeを分けます。

```text
/agent/v1/rpc
```

stable release前はold schemaとのcompatibilityを保証しません。Agent SyncにはAgent versionとcapabilityを含め、unsupported Commandを割り当てません。
