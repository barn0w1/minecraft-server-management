# Documentation index

このdirectoryはMinecraft Server Management Systemの**現在の設計の正本**です。current designはrepository内だけで理解できる状態を維持します。

## First reading path

1. [Vision](vision.md) — systemが提供する価値
2. [System model](system-model.md) — resource、process、lifecycleの全体像
3. [Scope](scope.md) — v1で実装するものとしないもの
4. [Terminology](terminology.md) — project内の正式な概念
5. [Design principles](design-principles.md) — 判断基準
6. [Architecture overview](architecture/overview.md) — authorityとdependency
7. [Domain overview](domains/README.md) — Minecraft Server、Server Home、Node
8. [Interfaces](interfaces/README.md) — Operator API、Agent API、local Minecraft control
9. [Plans](plans/README.md) — milestoneと実装順序

## Find information by question

| Question | Document |
| --- | --- |
| systemは何を実現するか | [`vision.md`](vision.md) |
| resourceとprocessはどう関係するか | [`system-model.md`](system-model.md) |
| 現在の対象範囲は何か | [`scope.md`](scope.md) |
| 用語の正確な意味は何か | [`terminology.md`](terminology.md) |
| system全体のauthorityは何か | [`architecture/overview.md`](architecture/overview.md) |
| moduleをどう分けるか | [`architecture/module-boundaries.md`](architecture/module-boundaries.md) |
| Operationとreconciliationはどう動くか | [`architecture/state-and-reconciliation.md`](architecture/state-and-reconciliation.md) |
| retryやunknown outcomeをどう扱うか | [`architecture/failure-model.md`](architecture/failure-model.md) |
| credentialとtrust boundaryは何か | [`architecture/security-model.md`](architecture/security-model.md) |
| Minecraft runtimeをどう扱うか | [`domains/minecraft-server.md`](domains/minecraft-server.md) |
| `/data`と設定をどうbackupするか | [`domains/server-home.md`](domains/server-home.md) |
| process間contractは何か | [`interfaces/`](interfaces/README.md) |
| なぜその判断をしたか | [`adr/`](adr/README.md) |
| どの順番で実装するか | [`plans/`](plans/README.md) |
| external specificationはどこか | [`references.md`](references.md) |

## Authoritative locations

| Information | Authority |
| --- | --- |
| purposeとnon-goal | [`vision.md`](vision.md)、[`scope.md`](scope.md) |
| resourceとend-to-end lifecycle | [`system-model.md`](system-model.md) |
| terminology | [`terminology.md`](terminology.md) |
| current architecture | [`architecture/`](architecture/overview.md) |
| domain invariant | [`domains/`](domains/README.md) |
| process boundary contract | [`interfaces/`](interfaces/README.md) |
| decision history | [`adr/`](adr/README.md) |
| milestoneとacceptance | [`plans/`](plans/README.md) |
| external primary references | [`references.md`](references.md) |

## Document types

### Current design

`vision.md`、`system-model.md`、`scope.md`、`terminology.md`、`design-principles.md`、`architecture/`、`domains/`、`interfaces/`はliving documentsです。

### Decision history

`adr/`は判断理由を保存します。current contractの詳細はcurrent design documentに置きます。

### Implementation plans

`plans/`は実装順序とacceptance criteriaを定義します。planはarchitectureの正本ではありません。

### Optional historical context

[`development/design-lineage.md`](development/design-lineage.md)は過去の設計から何を簡素化したかを説明します。current designの理解に必須ではありません。

## Writing policy

本文は日本語を基本にし、identifier、protocol field、official product名はEnglishを保ちます。一つの事実に一つの正本を持ち、overviewは詳細contractを複製せずlinkします。
