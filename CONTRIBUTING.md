# Contributing

このprojectは現在、architectureとcontractを先に確立するdesign-first段階です。

## Before changing anything

1. [`docs/index.md`](docs/index.md)で正本の配置を確認する
2. [`docs/terminology.md`](docs/terminology.md)で名称を確認する
3. 関連するarchitecture、domain、interface、ADR、planを読む
4. 変更が現在のscopeに必要か確認する

## Principles

- current designを簡潔に保つ
- speculative generalizationを追加しない
- external mutationの曖昧さを隠さない
- desired stateとobserved stateを区別する
- domain boundaryを越えてdatabase tableやexternal adapterを直接操作しない
- stable release前のprototype compatibilityより、現在の安全で単純な設計を優先する

Git運用、commit identity、bundle handoffは[`docs/development/repository-management.md`](docs/development/repository-management.md)および[`AGENTS.md`](AGENTS.md)を参照してください。
