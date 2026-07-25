# Documentation index

このdirectoryは、Minecraft Server Management Systemの**現在の設計の正本**です。

旧Python prototypeと旧`mc-control-plane` repositoryは、failure caseや設計経験の参照元ですが、ここへ逐語的には移植しません。新repositoryでは、現在も有効な知識だけを再評価し、簡潔なcontractとして記述します。

## Reading order

1. [Vision](vision.md)
2. [Scope](scope.md)
3. [Terminology](terminology.md)
4. [Design lineage](design-lineage.md)
5. [Design principles](design-principles.md)
6. [Architecture overview](architecture/overview.md)
7. [Domain overview](domains/README.md)
8. [Interfaces](interfaces/README.md)
9. [Foundation plan](plans/foundation.md)

## Authoritative locations

| Information | Authority |
| --- | --- |
| systemの目的と非目的 | [`vision.md`](vision.md)、[`scope.md`](scope.md) |
| 用語と命名 | [`terminology.md`](terminology.md) |
| 過去から継承した知見とreset範囲 | [`design-lineage.md`](design-lineage.md) |
| 現在のsystem構造 | [`architecture/`](architecture/overview.md) |
| domain conceptとinvariant | [`domains/`](domains/README.md) |
| process間contract | [`interfaces/`](interfaces/README.md) |
| 長期的な判断理由 | [`adr/`](adr/README.md) |
| 実装順序とmilestone | [`plans/`](plans/README.md) |
| repository運用 | [`development/`](development/repository-management.md) |
| 外部仕様の参照先 | [`references.md`](references.md) |

## Document types

### Current design

`architecture/`、`domains/`、`interfaces/`はliving documentsです。現在の実装が従うべき設計を直接説明します。

### Decision history

`adr/`は「なぜその方針を選んだか」を保存します。ADRだけを読まなくてもcurrent designを理解できる状態を維持します。

### Implementation plans

`plans/`は順序、slice、acceptance criteriaを定義します。planはarchitectureの正本ではありません。

## Writing policy

本文は日本語を基本にし、domain term、protocol名、identifierはEnglishを保ちます。同じ事実を複数のdocumentへ複製せず、他documentから正本へlinkします。
