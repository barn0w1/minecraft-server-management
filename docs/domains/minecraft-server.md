# Minecraft Server domain

## Purpose

Minecraft固有のdesired lifecycle、configuration、application state、safe operationを管理します。

## Owned concepts

- `MinecraftServer`
- `MinecraftServerSpec`
- `MinecraftServerStatus`
- `MinecraftServerOperation`
- server version and distribution
- application readiness
- player/world/server stateの必要なsubset

## Responsibilities

- Minecraft version、distribution、Java/runtime requirementの解釈
- server properties、mod/plugin inputなどのWorkload specへの変換
- application readinessの判定
- save、graceful stop、必要なserver-specific command
- Minecraft Server Management Protocol、RCON、server software固有integrationのcapability解釈
- local eventとobservationのdomain modelへの正規化

このdomainはMinecraftの仕様、command、internal ruleを深く知ってよい唯一の主要domainです。

## Control Plane and Node Agent split

Control Plane:

- desired stateとoperation orderingを決める
- save/stop/backup/releaseのpolicyを所有する
- durable operationとresultを追跡する

Node Agent:

- local control adapterを選ぶ
- Management Protocol/RCONへ接続する
- typed project operationをlocal protocol commandへ変換する
- local timeout、capability、process stateを観測する

## Adapter preference

initial preference:

1. Minecraft Server Management Protocol
2. server distribution固有のstructured integration
3. RCON
4. systemd/process-level graceful stop fallback

Control Plane protocolへofficial method名やRCON文字列をそのまま露出させません。

```text
project operation: minecraft.save
  → Node Agent adapter
  → Management Protocol method or RCON command
```

## Non-responsibilities

- Akamai API
- restic repository implementation
- Podman command construction
- filesystem backup retention
- Node identity enrollment
