# Repository management

## Repository identity

```text
Repository: minecraft-server-management
Default branch: main
```

## Commit identity

すべてのcommitのAuthorとCommitterを次に固定します。

```text
barn0w1 <yuito.kiuchi.dev@gmail.com>
```

local configuration:

```bash
git config user.name barn0w1
git config user.email yuito.kiuchi.dev@gmail.com
```

AI agent、automation、別のhuman nameをcommit author、committer、co-author trailerへ追加しません。

## Commit policy

- 一つのcommitは一つのlogical change
- imperativeで短いsubjectを使用する
- prefix例: `docs:`, `chore:`, `feat:`, `fix:`, `test:`
- generated artifact、secret、bundleをrepositoryへcommitしない
- history rewrite、force push、commit amendは明示要求がない限り行わない
- current designに矛盾するobsolete textを残さない

## Branches

初期のsmall-team運用では`main`を常に整合した状態に保ちます。作業branchを使う場合も、merge後にdocumentation linkとvalidationが通ることを要求します。release branchやlong-lived integration branchは必要になるまで作りません。

## Bundle handoff

ChatGPT/AI agent経由の作業では、commit後に完全Git bundleを作成します。

```bash
git bundle create minecraft-server-management-$(git rev-parse --short HEAD).bundle main
git bundle verify minecraft-server-management-$(git rev-parse --short HEAD).bundle
```

別の空repositoryで次を検証します。

```bash
git init verify-repo
git -C verify-repo fetch ../minecraft-server-management-<hash>.bundle main:main
git -C verify-repo rev-parse main
```

bundle fileはhandoff artifactでありGit treeへ追加しません。

## Tags and releases

最初のstable contractまではrelease tagを急ぎません。tagを使用する場合はannotated tagとし、release noteにdatabase/protocol compatibilityを明記します。

## Secrets and local files

次をcommitしません。

- `.env`
- cloud credential
- private key/certificate key
- enrollment token
- real account inventory/evidence
- SQLite runtime database
- acceptance test artifact
- generated bundle

必要なexample configurationは架空値と明確なplaceholderを使用します。
