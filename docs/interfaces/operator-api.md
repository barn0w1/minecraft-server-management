# Operator API

## Purpose

Operator clientがControl Planeのdesired state、query、Operation、Incidentを操作するlocal APIです。

## Clients

- `mcserverctl`
- Discord Bot
- local automation

全clientはdatabase、Akamai Cloud、Node Agentへ直接接続しません。

## Transport

initial contract:

```text
JSON-RPC 2.0
  over HTTP/2 prior knowledge
  over Unix domain socket
```

socket:

```text
/run/mcserver/control-plane.sock
```

TLSは使用せず、filesystem permissionとUnix peer credentialをauthentication boundaryとします。HTTP version/libraryはimplementation前に再検証できますが、public TCP endpointにはしません。

## Authorization

初期実装ではsocket accessを持つclientはfull-control Operatorです。Discord userごとのauthorizationはDiscord Bot側で行います。将来Control Plane側でfine-grained authorizationが必要になった場合は、別ADRとidentity modelを追加します。

## JSON-RPC profile

- versionは`2.0`だけ
- request/response IDはcanonical UUIDv7 stringを推奨
- `params`はnamed object
- batchは初期実装で無効
- unknown fieldは拒否
- errorはstableな`data.kind`を持つ
- long-running mutationはdurable Operation IDを返す
- timeoutはoperationが未実行だったことを意味しない

## Method namespaces

候補となるtop-level namespace:

```text
server.*
node.*
backup.*
restore.*
operation.*
incident.*
```

exact methodとschemaは各milestoneで追加します。CLI commandとRPC methodを一対一に固定せず、CLIは複数queryを組み合わせてhuman-readable表示を作れます。

## Audit actor

Control Planeが観測できるUnix peer identityをauthentication actorとして保存します。BotはDiscord user metadataを追加できますが、これはtrusted clientが申告したaudit contextであり、Control PlaneがDiscord userを直接認証した証拠ではありません。
