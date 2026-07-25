# Documentation style and maintenance

## Language

- filenameはEnglish kebab-case
- 本文は日本語を基本とする
- identifier、protocol field、official product/protocol nameはEnglish
- `Node`、`Workload`、`Controller`など定義済みtermを無理に日本語化しない

## Authority and duplication

一つの事実には一つの正本を持たせます。

```text
Why QUIC?              → ADR
How Agent Protocol works? → interfaces/agent-protocol.md
When to implement it?  → plans/
```

同じtimeout、method list、state transitionをrequirements/architecture/ADR/planへ複製しません。別documentでは正本へlinkします。

## Normative language

contractでは次を意識して使い分けます。

- `MUST`: safety/interoperability上必須
- `MUST NOT`:禁止
- `SHOULD`: strong defaultだが例外を許す
- `MAY`: optional
- `initial default`: implementation/measurementで変更可能な値

日本語本文でもkeywordをEnglishで残して構いません。

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

命名や小さなdefault値をすべてADRにしません。

## Validation

- relative Markdown link
- heading hierarchy
- code block fence
- terminology consistency
- duplicate current contract
- resolved review annotationの残存
- stale planとcurrent designの混同
