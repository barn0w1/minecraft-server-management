# Minecraft Server control interface

## Boundary

```text
Control Plane MinecraftServer module
  ↕ typed Agent Command
Node Agent minecraft/runtime capability
  ↕ local system and RCON
itzg/minecraft-server container
```

Control PlaneはRCONへ直接接続しません。Node Agentがendpoint、password file、response parsing、runtime stateを吸収します。

## Runtime operations

```text
server_home.prepare
server_home.observe
server.runtime.apply
server.runtime.start
server.runtime.stop
server.runtime.observe
server.runtime.remove
```

`server.runtime.apply`はSpec generationとServer Home manifestをQuadletへmaterializeします。

## Minecraft operations

```text
minecraft.observe
minecraft.players.get
minecraft.save.prepare
minecraft.save.resume
minecraft.stop
```

exact params/result schemaはLocal Node milestoneで定義します。

## RCON contract

- RCONはitzg/minecraft-serverのlocal endpointへ接続する
- passwordはServer Homeのfileから読む
- RCON portをpublic networkへpublishしない
- raw command stringをControl Planeから受け取らない
- known response patternをtyped resultへ変換する

## Readiness

Node Agentは次をまとめてObservationとして返します。

```text
systemd active state
container running state
container health state
resolved image digest
manifest generation
RCON query success
player count if available
```

Control PlaneがConditionを導出します。Agentはglobal `Ready` policyを決めません。

## Graceful stop

initial strategy:

1. typed `minecraft.stop`をRCONで実行する
2. systemd/Podman observationでprocess停止を待つ
3. deadline超過時はOperationをDegraded/FailedとしてControl Planeへ返す
4. force stopは通常pathと分離したexplicit Commandにする

itzg container/systemd lifecycleが安全なstopを提供する場合でも、停止完了はlocal process observationで確認します。

## Online backup save sequence

```text
minecraft.save.prepare
  → save-off
  → save-all flush

restic backup

minecraft.save.resume
  → save-on
```

`save.resume`はfinally behaviorとして実行します。backup successの判定自体はrestic resultに従います。

## Safety

- unsupported Commandは実行しない
- stale Fencing TokenのCommandは拒否する
- arbitrary RCON proxyを提供しない
- runtime configurationにarbitrary host mount、privileged mode、host commandを許可しない
