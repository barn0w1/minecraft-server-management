# Operator API

## Purpose

Operator ClientがMinecraftServer Spec、query、Operation、Snapshot、Incidentを操作するlocal APIです。

## Clients

- `mcserverctl`
- Discord Bot
- local automation

全clientはSQLite、Akamai、Node Agent、RCONへ直接接続しません。

## Transport

```text
JSON-RPC 2.0
  over HTTP/2 prior knowledge
  over Unix domain socket
```

socket:

```text
/run/mcserver/control-plane.sock
```

TLSは使用せず、filesystem permissionとUnix peer credentialをauthentication boundaryとします。

## JSON-RPC profile

- `jsonrpc`は`"2.0"`
- request IDはcanonical UUID string
- `params`はobject
- batchとnotificationはv1で使用しない
- unknown fieldはschema validation error
- stable errorは`error.data.kind`を持つ
- long-running mutationはOperationを返す
- client timeoutはOperation未作成を意味しないため、request IDまたはidempotency keyで再照会できる

## Method namespaces

initial namespace:

```text
system.*
server.*
node.*
snapshot.*
operation.*
incident.*
```

representative methods:

```text
system.version
server.create
server.get
server.list
server.spec.update
server.start
server.stop
server.backup
server.restore
node.get
node.list
operation.get
operation.list
operation.cancel
snapshot.list
incident.list
incident.resolve
```

exact schemaは各milestoneで追加します。

## Mutation semantics

mutation requestにはclient-generated `request_key`を持たせます。Control Planeは同じactor、method、request key、payload hashに対して同じOperationまたはresultを返せます。

JSON-RPC IDはtransport correlationであり、mutation idempotency keyではありません。

## Status response

`server.get`は少なくとも次を返せるようにします。

```text
desired state
spec generation
current phase
Conditions
active Node
Agent freshness
applied generation
runtime state
latest successful Snapshot
active Operation and stage
next retry time
last error summary
```

## Authorization and audit

socket accessを持つclientはfull-control Operatorです。Control PlaneはUnix peer identityをauthentication actorとして保存します。

Discord BotはDiscord user metadataをaudit contextとして追加できますが、Control PlaneがそのDiscord userを直接認証した証拠とはみなしません。
