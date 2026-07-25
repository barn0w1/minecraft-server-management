# Documentation style and maintenance

## Reader model and self-containment

読者は、このsystem、過去のconversation、旧repository、prototypeを知らないことを前提にします。優秀なengineerがrepositoryだけを読んで正確なmental modelを構築できることが目標です。

- current designはrepository内だけで理解できるようにする
- 「前述の議論」「以前の設計」「旧checkpoint」のような外部contextを要求しない
- project固有termは初出で説明するか[`terminology.md`](../terminology.md)へlinkする
- documentの冒頭でpurpose、対象boundary、必要なprerequisiteを明確にする
- optionalなhistoryとnormativeなcurrent contractを混ぜない
- 未決事項を過去の記憶から補完せず、状態を`Proposed`、`P0 decision`、`initial default`などで示す

Historical contextは[`design-lineage.md`](design-lineage.md)に限定し、first reading pathやcurrent contractの前提にしません。

## Language

- filenameはEnglish kebab-case
- 本文は日本語を基本とする
- identifier、protocol field、official product/protocol nameはEnglish
- `Node`、`Workload`、`Controller`など定義済みtermを無理に日本語化しない

## Authority and duplication

一つの事実には一つの正本を持たせます。

```text
Why QUIC?                  → ADR
How Agent Protocol works?  → interfaces/agent-protocol.md
When to implement it?      → plans/
```

同じtimeout、method list、state transitionをrequirements、architecture、ADR、planへ複製しません。別documentでは正本へlinkします。

Overviewは詳細contractを複製せず、詳細文書へ到達するためのnarrativeとdependencyを提供します。

## Normative language

contractでは次を意識して使い分けます。

- `MUST`: safety/interoperability上必須
- `MUST NOT`: 禁止
- `SHOULD`: strong defaultだが例外を許す
- `MAY`: optional
- `initial default`: implementation/measurementで変更可能な値

日本語本文でもkeywordをEnglishで残して構いません。

## Document status

current document内で確定度が重要な場合は明示します。

- `Accepted`: architecture decisionとして採用済み
- `Proposed`: review前または未確定
- `P0 decision`: implementation開始前に解決必須
- `initial direction`: directionは採用しているがschemaやdefaultが未確定

曖昧な`TBD`を増やさず、何が未決で、どのmilestoneをblockするかを書きます。

## Review annotations

```text
> [REVIEW:YUITO][QUESTION][TP-001]
```

Type:

- `QUESTION`
- `CHANGE`
- `DISAGREE`
- `RISK`
- `APPROVE`
- `NOTE`

annotation解決時は本文へ判断を統合し、annotation自体は削除します。

## Diagrams

Mermaidまたはplain textを使用し、diagramだけを正本にしません。identity、authority、dependency directionは文章でも記述します。

## ADR threshold

次の場合だけADRを作ります。

- 複数の合理的な選択肢がある
- 長期的architecture impactがある
- 変更costが大きい
- 後から理由が失われやすい

命名や小さなdefault値をすべてADRにしません。ADRは判断理由を保存し、current interfaceやdomain contractの詳細を複製しません。

## Validation

- relative Markdown link
- heading hierarchy
- code block fence
- terminology consistency
- duplicate current contract
- resolved review annotationの残存
- stale planとcurrent designの混同
- repository外のcontextを要求する表現
- optional historyがnormative authorityとして参照されていないこと
