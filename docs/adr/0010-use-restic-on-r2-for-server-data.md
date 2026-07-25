# ADR-0010: Use restic repositories on Cloudflare R2 for Server Data

Status: Accepted

## Context

Nodeを交換可能にするには、Minecraft Serverのpersistent file treeをCompute Instance lifecycleから分離する必要があります。

resticはSnapshot、deduplication、repository integrity checking、複数backendを提供します。restic repository formatはdataとmetadataを暗号化・認証し、encryptionを無効化するmodeはありません。一方、このsystemのrepository confidentialityはR2 access controlを主要boundaryとし、別のrestic password secretを運用しない方針です。

## Decision

- 永続dataを共有する一つのlogical Minecraft Serverごとに一つのrestic repositoryを作成する
- 一つのrepository内に複数のSnapshotを保持する
- initial backendとしてCloudflare R2のS3-compatible endpointを使用する
- restic repository passwordは**空文字列**とする
- repositoryの作成、backup、restore、check、forget、pruneなど、すべてのrestic invocationでempty passwordへの明示的opt-inを使用する
- restic password file、password environment variable、password secretを作成・保存しない

現在のresticではempty password repositoryを扱うために`--insecure-no-password`を明示する必要があります。Node Agentのrestic adapterは、このflagをcommandごとに確実に付与し、password入力と同時指定を行いません。

## Security consequences

resticの暗号化・認証repository format自体は、empty passwordでも使用されます。しかしempty passwordは秘密ではないため、repository objectをreadできる主体に対するconfidentiality boundaryにはなりません。

したがって、次を主要security boundaryとします。

- R2 credentialのleast privilege
- bucket/prefix isolation
- credentialを取得できるprocessとNodeの制限
- unauthorized read/writeを防ぐCloudflare account policy
- destructive repository operationをControl Planeのdurable operationとauthorizationで制御すること

repositoryをreadできる主体はempty passwordを知っているものとして扱います。repositoryへwriteできる悪意ある主体に対して、resticのauthenticationだけを改ざん防止boundaryとはみなしません。

## Operational consequences

利点:

- repository passwordの生成、配布、rotation、backup、loss recoveryが不要になる
- password secret喪失によってSnapshotが永久にrestore不能になるriskを避けられる
- Node replacement時に必要なsecretをR2 credentialへ集約できる

負担:

- `--insecure-no-password`の付与漏れはoperation failureになるため、typed adapterとtestで保証する必要がある
- R2 credential compromiseはrepository confidentialityへ直結する
- repository check、restore verification、retention、pruneを別途管理する必要がある

## Related documents

- [Server Data domain](../domains/server-data.md)
- [Security model](../architecture/security-model.md)
- [References](../references.md#server-data)
