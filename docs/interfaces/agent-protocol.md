# Agent Protocol

## Purpose

Control PlaneとNode Agentの間で、authenticatedなtyped request/response、notification、observationを交換します。

## Layering

```text
Domain RPC methods
  → JSON-RPC 2.0 project profile
  → length-prefixed UTF-8 JSON frame
  → QUIC stream
  → QUIC v1 / TLS 1.3
  → UDP 443
```

HTTP/3は使用しません。

## Endpoint and connection

- Node Agentがstable DNS endpointへoutbound接続する
- Control Plane server certificateはendpointのDNS SANを持つ
- enrollment後はmTLSを要求する
- 一つのactive Agent sessionにつき一つのlong-lived QUIC connection
- AgentとControl Planeの双方がbidirectional streamを開始できる
- remote IP/portはNode identityではない
- validated connection migration/NAT rebindingをidentity changeとして扱わない
- 0-RTT application dataを使用しない

## Stream mapping

### Request/response

一つのbidirectional streamに一つのrequestと一つのresponseを対応させます。

```text
request initiator  ── request + FIN ──> receiver
request initiator  <─ response + FIN ── receiver
```

streamを再利用しません。

### Notification

許可されたnotificationは、一つのunidirectional streamへ一件だけ送ります。initial notificationは`agent.heartbeat`です。

QUIC DATAGRAMは初期実装で使用しません。reliable streamと同じframing/parserを使用し、loss handlingの種類を増やさないためです。

## Framing

各stream方向のframe:

```text
u32 big-endian payload length
UTF-8 JSON bytes
FIN
```

次をprotocol violationとして扱います。

- declared lengthより短いpayload
- size limit超過
- payload後のtrailing data
- 一方向に複数message
- invalid UTF-8またはBOM
- duplicate JSON member
- top-level array/scalar
- JSON-RPC batch

有限payload limit、stream limit、connection flow-control、handler concurrency、deadlineを必須とします。exact defaultはimplementation planでbenchmark前提のinitial valueとして定めます。

## JSON-RPC profile

- `jsonrpc`は`"2.0"`
- request IDはcanonical lowercase UUIDv7 stringとし、number/null IDを禁止する
- `params`はobject
- methodはlowercase namespaced string
- `result`と`error`は排他的
- notificationは明示allowlistだけ
- unknown methodはstandard JSON-RPC errorへmapping
- application errorはstableな`data.kind`と`retryable`を持つ

## Method families

Agent core:

```text
agent.enroll
agent.heartbeat
agent.report
agent.certificate.rotate
```

Domain capability:

```text
node.*
workload.*
server_data.*
minecraft.*
```

Minecraft公式protocolやRCON commandをこのwire contractへ直接露出させません。

## Heartbeat and report

### Heartbeat

- JSON-RPC notification
- Agent-initiated unidirectional stream
- `session_id`とmonotonic `sequence`
- Node identityはmTLS connection contextから取得
- freshnessはControl Plane受信時刻で判定
- heartbeat一件ごとにdatabaseへwriteしない

### Full report

request/responseとして、次の場合に送ります。

- active connection確立直後
- boot/process/capability/health変更
- Control Planeからの要求
- periodic refresh

reportはheartbeatより低頻度です。

## Reconnect

unexpected connection loss後、Node Agentは自律的に再接続します。

initial nominal schedule:

```text
1, 2, 4, 8, 16, 32, 60, 60... seconds
```

actual delayには50–100%程度のjitterを加えます。mTLS connectionとinitial report successの両方を確認した場合だけbackoffをresetします。

## Session replacement

同じNode identityの新sessionをacceptした場合、旧sessionを明示的にcloseし、旧connection由来のheartbeat/reportをactive stateへ反映しません。

## Delivery semantics

- response未受信はmethod未実行を意味しない
- stream resetはoperation rollbackを意味しない
- JSON-RPC IDはcorrelationでありidempotency keyではない
- mutation methodはdurable Operation IDまたはmethod固有idempotency identityを使用する
- orderingはdomain sequence/generationで表し、stream arrival orderへ依存しない

## Protocol versioning

ALPNでincompatible wire protocolを分けます。

```text
mcserver-enroll/1
mcserver-agent/1
```

method-level capabilityとschema versionはinitial report/capability exchangeで扱います。古いprototypeのALPN名とのcompatibilityは提供しません。
