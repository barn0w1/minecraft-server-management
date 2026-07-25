# Minecraft Server control interface

## Boundary

```text
Control Plane Minecraft module
  ↕ project Agent RPC
Node Agent Minecraft module
  ↕ local protocol
Minecraft Server process
```

Control PlaneはMinecraft公式protocolへ直接接続しません。Node Agentがlocal endpoint、protocol version、RCON、process stateを吸収します。

## Project operations

project側ではdomain-orientedなoperationを定義します。

```text
minecraft.observe
minecraft.save
minecraft.stop
minecraft.players.get
minecraft.operation.get
```

exact schemaはMinecraft Server milestoneで定義します。

## Local adapters

### Minecraft Server Management Protocol

structuredなJSON-RPC over WebSocket interfaceとして、supported method/notificationをcapability discoveryできます。Node内のloopbackへbindし、外部networkへ直接公開しない方針です。

### RCON

Management Protocolが利用できないversion/distribution向けfallbackです。command stringとtext responseをNode Agent内でtyped resultへ正規化します。

### Process/systemd fallback

application protocolが利用できない場合の最後のfallbackです。Minecraft固有のsafe saveを証明できない場合、強制停止を通常の成功pathにしません。

## Capability discovery

Node Agentは起動/接続時に利用可能なadapter、protocol version、supported operationを観測し、Control Planeへcapabilityとして報告します。Control Planeはunsupported operationを送信しません。

## Safety

- save completionを単なるrequest send成功とみなさない
- stop request後にprocess/workload observationで停止を確認する
- response loss時に同じnon-idempotent operationをblind retryしない
- terminal resultはnotificationではなくrequest/responseまたはdurable queryで確認する
- progress/event notificationはauthoritative completionとして扱わない
