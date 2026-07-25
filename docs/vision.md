# Vision

## Purpose

Minecraft Server Management Systemは、小規模なcommunityがMinecraft Java serverを**日常的に迷わず運用できるself-hosted automation system**です。

Operatorは次だけを表現します。

- どのMinecraft Serverを動かしたいか
- どのversion、distribution、設定で動かしたいか
- いつ停止、backup、restore、updateしたいか
- Nodeを常時維持するか、必要時に作るか

SystemはNode provisioning、Server Home restore、itzg runtime適用、readiness、graceful stop、backup、Node releaseを自動的に協調させます。

## Product goal

```text
Operator intent
  → durable Operation
  → automatic reconciliation
  → observable result
```

通常のnetwork failure、process restart、Agent reconnect、provider一時障害は、人間が毎回介入しなくても回復するべきです。一方、誤ったresource削除につながるidentity contradictionなど、systemが安全な選択をできない場合だけOperatorへ判断を求めます。

## Priorities

1. 日常操作が簡単であること
2. 通常の一時障害から自動回復すること
3. Nodeを失ってもServer Homeを復元できること
4. 何を実行中で、なぜ待っているか説明できること
5. 破壊的な操作を明示的な条件で制御すること
6. 実際の用途以上に一般化しないこと

完全なfailure proof、enterprise-grade security、すべてのexternal inconsistencyの独自検証は目的ではありません。restic、Cloudflare R2、systemd、Podman、itzg/minecraft-serverなど、採用したsoftwareのdocumented contractを信頼し、その上に必要なautomationだけを構築します。

## Core idea

```text
Minecraft Serverが主役
Server Homeは復元可能
Nodeは交換可能
Operationが処理を追跡する
```

Cloud VM、container、Java processをMinecraft Serverそのものとはみなしません。logical server identityはControl Planeが所有し、実行場所は必要に応じて交換できます。

## Intended users

- 一つまたは少数のMinecraft community
- 少人数のtrusted Operator
- 一つのControl Plane deployment
- infrastructureとbackup storageを自分たちで所有するself-hosted運用
