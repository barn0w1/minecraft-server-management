# Vision

## Purpose

Minecraft Server Management Systemは、小規模なcommunityが自分たちのMinecraft serverを便利に、安全に、堅固に運用するためのself-hosted management systemです。

operatorがserverごとのinfrastructure、process、persistent data、Minecraft固有operationを個別のmanual procedureとして扱うのではなく、一つのdesired lifecycleとして操作できることを目指します。

```text
MinecraftServer desired state
        ↓
Nodeの確保
        ↓
Server Dataのrestore
        ↓
Workloadの起動
        ↓
Minecraft application readiness
```

停止時には逆向きに、安全なsave、graceful stop、backup、snapshot verification、Node releaseをorchestrateします。

## Intended users

- 自分たちのMinecraft serverを管理する少人数のoperator
- 一つまたは少数のcommunity
- 基本的に一つのControl Plane deployment
- infrastructureとdataを自分たちで所有するself-hosted運用

## Values

優先順位は次です。

1. dataを失わないこと
2. 誤ったresourceを変更・削除しないこと
3. 不明な状態を成功として扱わないこと
4. process restartや一時的なnetwork failureから再収束できること
5. operatorが現在状態と停止理由を説明できること
6. 日常運用が簡単であること
7. 実際の必要性に応じて拡張できること

小規模であることは、identity、ownership、durability、failure safetyを省略する理由にはしません。一方で、enterprise向けの一般性や無制限な拡張性のためにsystemを複雑化しません。

## Core idea

```text
Nodeは交換可能
Server Dataは永続
Minecraft Server lifecycleが両者を協調させる
```

Cloud VMの存在をMinecraft serverそのものとはみなしません。logical identity、workload、persistent data、application stateを分離して管理します。
