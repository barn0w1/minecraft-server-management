# AGENTS.md

このfileは、このrepositoryで作業するAI agentとautomation向けの拘束的な作業規則です。

## Project authority

- 現在のrepositoryと`docs/`が正本です。
- 旧Python prototypeと旧`mc-control-plane` repositoryは歴史的な参考資料であり、code、schema、名称、module構造、documentをそのまま移植しません。
- 過去の設計からは、failure case、不変条件、実環境で得た知見だけを抽出します。
- stable release前は、古いprototypeとの後方互換性を目的にcompatibility layerを追加しません。

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

commit前に`git show -s --format=fuller HEAD`または作成したcommitのmetadataを確認します。AI agent名、共同著者、生成tool名をcommit trailerへ追加しません。

## Change discipline

- taskに必要な変更だけを行います。
- implementationを要求されていないtaskでは、source code、Cargo workspace、database migration、deployment artifactを追加しません。
- document変更では、同じ事実を複数fileへ複製せず、正本を一つにします。
- architecture上の長期的判断を変更する場合は、current design documentとADRを同じchangeで更新します。
- planは現在のarchitectureの正本ではありません。planから恒久的な設計が生じた場合は、先にarchitectureまたはinterface documentへ反映します。
- obsoleteな説明を残して追記だけで矛盾を解消しません。現在の正本を直接cleanに書き換えます。
- secret、credential、private key、token、real account ID、private endpointをcommitしません。

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
- duplicate headingや矛盾したterminologyがないこと
- `git status --short`で意図しないfileがないこと
- commit metadataがrequired Git identityと一致すること

## Bundle handoff

変更をcommitした場合は、`main`を含む完全bundleを次の名前で作成します。

```text
minecraft-server-management-<short-commit>.bundle
```

例:

```bash
git bundle create minecraft-server-management-$(git rev-parse --short HEAD).bundle main
git bundle verify minecraft-server-management-$(git rev-parse --short HEAD).bundle
```

bundleから別repositoryへfetchでき、`main`のcommitが一致することまで検証します。
