# AGENTS.md

このfileは、このrepositoryで作業するAI agentとautomation向けの拘束的な作業規則です。

## Reader model

作業開始時点で、このsystemに関するconversation history、旧repository、prototype、暗黙の前提を一切持っていないものとして行動します。

- current repository内のdocumentだけをsource of truthとして使用する
- userがこのtaskで明示的に提供した情報以外の会話や過去artifactを前提にしない
- 「以前決めた」「既知の通り」のような外部contextをcurrent documentへ持ち込まない
- documentに定義されていない設計を、過去実装や一般的な慣習から確定事項として推測しない
- 不明点は、既存のauthority document、ADR、planを調査し、それでも未定義ならopen decisionとして明示する

初めて作業するagentは、最低限次を順に読みます。

1. [`README.md`](README.md)
2. [`docs/index.md`](docs/index.md)
3. [`docs/system-model.md`](docs/system-model.md)
4. [`docs/terminology.md`](docs/terminology.md)
5. taskに関係するarchitecture、domain、interface、ADR、plan

## Project authority

- `docs/`がsystem designの正本です。
- current designは`vision.md`、`system-model.md`、`scope.md`、`terminology.md`、`design-principles.md`、`architecture/`、`domains/`、`interfaces/`にあります。
- ADRは判断理由の記録であり、current contractの詳細な正本ではありません。
- planは実装順序とacceptanceを定義しますが、architectureの正本ではありません。
- optionalなhistorical contextは[`docs/development/design-lineage.md`](docs/development/design-lineage.md)にあります。current designの理解や実装に必須ではありません。
- stable release前は、旧prototypeとの後方互換性を目的にcompatibility layerを追加しません。

## Required Git identity

すべてのcommitでAuthorとCommitterを次に固定します。

```text
barn0w1 <yuito.kiuchi.dev@gmail.com>
```

作業repositoryでは少なくとも次を設定します。

```bash
git config user.name barn0w1
git config user.email yuito.kiuchi.dev@gmail.com
```

commit後にmetadataを確認します。

```bash
git show -s --format=fuller HEAD
```

AI agent名、共同著者、生成tool名をcommit trailerへ追加しません。

## Change discipline

- taskに必要な変更だけを行います。
- implementationを要求されていないtaskでは、source code、Cargo workspace、database migration、deployment artifactを追加しません。
- document変更では、同じ事実を複数fileへ複製せず、正本を一つにします。
- architecture上の長期的判断を変更する場合は、current design documentとADRを同じchangeで更新します。
- planから恒久的な設計が生じた場合は、architecture、domain、interfaceの適切な正本へ反映します。
- obsoleteな説明を残したまま追記で矛盾を隠しません。current textをcleanに書き換えます。
- historical documentへcurrent contractを置きません。
- secret、credential、private key、token、real account ID、private endpointをcommitしません。

## Documentation requirements

- documentは、このsystemを知らない優秀なengineerが単独で読んで理解できるように書きます。
- documentの冒頭で、扱う対象と境界を明確にします。
- uncommonなproject termは[`docs/terminology.md`](docs/terminology.md)で定義するか、初出で説明します。
- external conceptの詳細を再説明しすぎず、必要なcontractを本文に書き、primary referenceへlinkします。
- 「この前の設計」「旧checkpoint」「会話で決めた」など、repository外のcontextを要求する表現を使用しません。
- uncertainな設計を確定形で書きません。`Proposed`、`initial default`、`P0 decision`などで状態を明示します。

## Language and naming

- filename、code identifier、protocol method、configuration keyはEnglishを使用します。
- document本文は可読性のため日本語を基本とし、一般的なtechnical termは無理に翻訳しません。
- project terminologyは[`docs/terminology.md`](docs/terminology.md)へ従います。
- `Host`と`Node`、`Manager`と`Controller`などの同義語を混在させません。

## Documentation review annotations

inline reviewには次の形式を使用できます。

```text
> [REVIEW:YUITO][TYPE][TARGET-ID]
```

`TYPE`は`QUESTION`、`CHANGE`、`DISAGREE`、`RISK`、`APPROVE`、`NOTE`のいずれかです。annotationを解決するcommitでは、本文へ判断を反映したうえでannotationを削除します。Git historyが議論の記録になります。

## Validation

少なくとも次を確認します。

- `git diff --check`
- Markdownのrelative linkが存在すること
- heading hierarchyとcode fenceが正しいこと
- duplicate current contractや矛盾したterminologyがないこと
- repository外のcontextを要求する表現がcurrent designにないこと
- `git status --short`で意図しないfileがないこと
- commit metadataがrequired Git identityと一致すること

source codeを変更した場合は、taskに応じたformat、lint、testも実行します。

## Bundle handoff

変更をcommitした場合は、`main`を含む完全bundleを次の名前で作成します。

```text
minecraft-server-management-<short-commit>.bundle
```

```bash
git bundle create minecraft-server-management-$(git rev-parse --short HEAD).bundle main
git bundle verify minecraft-server-management-$(git rev-parse --short HEAD).bundle
```

bundleから別repositoryへfetchでき、`main`のcommitが一致することまで検証します。
