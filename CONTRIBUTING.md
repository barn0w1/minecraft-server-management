# Contributing

このprojectは現在、architectureとcontractを先に確立するdocumentation-first段階です。

Contributorがこのsystemや過去の議論を知っていることは前提にしません。repository内のdocumentationだけで変更理由と影響範囲を判断できる状態を維持します。

## Before changing anything

1. [`README.md`](README.md)でsystemの目的と現在状態を確認する
2. [`docs/index.md`](docs/index.md)で正本の配置を確認する
3. [`docs/system-model.md`](docs/system-model.md)でresource、process、lifecycleを理解する
4. [`docs/terminology.md`](docs/terminology.md)で名称を確認する
5. taskに関係するarchitecture、domain、interface、ADR、planを読む
6. 変更が現在のscopeとmilestoneに必要か確認する

## Principles

- current designを自己完結かつ簡潔に保つ
- repository外の会話や旧implementationを前提にしない
- speculative generalizationを追加しない
- external mutationの曖昧さを隠さない
- desired stateとobserved stateを区別する
- domain boundaryを越えてdatabase tableやexternal adapterを直接操作しない
- stable release前のprototype compatibilityより、現在の安全で単純な設計を優先する
- 設計変更ではcurrent documentと必要なADRを同時に更新する

Git運用、commit identity、bundle handoffは[`docs/development/repository-management.md`](docs/development/repository-management.md)および[`AGENTS.md`](AGENTS.md)を参照してください。
