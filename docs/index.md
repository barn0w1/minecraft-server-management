# Documentation index

このdirectoryは、Minecraft Server Management Systemの**現在の設計の正本**です。

読者は、このsystem、過去の会話、以前のprototype、旧repositoryについて何も知らないことを前提にしています。current designを理解するために外部の履歴を読む必要はありません。文書だけで目的、用語、resource、process、authority、failure model、interface contractを再構築できる状態を維持します。

## First reading path

1. [Vision](vision.md) — 何のためのsystemか
2. [System model](system-model.md) — 何を管理し、processとdomainがどう協調するか
3. [Scope](scope.md) — 何を含み、何を含まないか
4. [Terminology](terminology.md) — resourceとcomponentの正式名称
5. [Design principles](design-principles.md) — 長期的な判断基準
6. [Architecture overview](architecture/overview.md) — dependency、authority、process boundary
7. [Domain overview](domains/README.md) — Node、Workload、Server Data、Minecraft Server
8. [Interfaces](interfaces/README.md) — Operator APIとAgent Protocol
9. [Foundation plan](plans/foundation.md) — 実装開始時の順序

この順序は必須のprerequisite chainではありませんが、初見の読者に最短で共通mental modelを作ります。

## Find information by question

| Question | Document |
| --- | --- |
| systemは何を実現するか | [`vision.md`](vision.md) |
| resourceとprocessはどう関係するか | [`system-model.md`](system-model.md) |
| 現在の対象範囲は何か | [`scope.md`](scope.md) |
| 用語の正確な意味は何か | [`terminology.md`](terminology.md) |
| system全体のdependencyとauthorityは何か | [`architecture/overview.md`](architecture/overview.md) |
| processはどこで動き、何を所有するか | [`architecture/process-model.md`](architecture/process-model.md) |
| state、reconciliation、restart recoveryはどう動くか | [`architecture/state-and-reconciliation.md`](architecture/state-and-reconciliation.md) |
| failureとIncidentをどう分類するか | [`architecture/failure-model.md`](architecture/failure-model.md) |
| trust boundary、PKI、secretはどう扱うか | [`architecture/security-model.md`](architecture/security-model.md) |
| domainごとの責務とnon-responsibilityは何か | [`domains/`](domains/README.md) |
| process間protocolは何か | [`interfaces/`](interfaces/README.md) |
| なぜそのarchitectureを選んだか | [`adr/`](adr/README.md) |
| どの順番で実装するか | [`plans/`](plans/README.md) |
| repositoryをどう管理するか | [`development/`](development/repository-management.md) |
| external specificationはどこにあるか | [`references.md`](references.md) |

## Authoritative locations

| Information | Authority |
| --- | --- |
| systemの目的と非目的 | [`vision.md`](vision.md)、[`scope.md`](scope.md) |
| system resourceとend-to-end lifecycle | [`system-model.md`](system-model.md) |
| 用語と命名 | [`terminology.md`](terminology.md) |
| 現在のsystem構造 | [`architecture/`](architecture/overview.md) |
| domain conceptとinvariant | [`domains/`](domains/README.md) |
| process間contract | [`interfaces/`](interfaces/README.md) |
| 長期的な判断理由 | [`adr/`](adr/README.md) |
| 実装順序とmilestone | [`plans/`](plans/README.md) |
| repository運用 | [`development/`](development/repository-management.md) |
| 外部仕様の参照先 | [`references.md`](references.md) |

## Document types

### Current design

`vision.md`、`system-model.md`、`scope.md`、`terminology.md`、`design-principles.md`、`architecture/`、`domains/`、`interfaces/`はliving documentsです。現在の実装が従うべき設計を直接説明します。

### Decision history

`adr/`は「なぜその方針を選んだか」を保存します。ADRを読まなくてもcurrent designを理解できる状態を維持します。

### Implementation plans

`plans/`は実装順序、scope、acceptance criteriaを定義します。planはarchitectureの正本ではありません。

### Optional historical context

[`development/design-lineage.md`](development/design-lineage.md)は、このrepositoryより前の試行から何を学び、何をresetしたかを説明します。current designの理解や変更に必須ではなく、normative contractでもありません。

## Writing policy

本文は日本語を基本にし、domain term、protocol名、identifierはEnglishを保ちます。文書は過去の会話を参照せず自己完結させ、同じ事実を複数documentへ複製せず正本へlinkします。
