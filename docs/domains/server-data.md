# Server Data domain

## Purpose

Minecraft Serverの完全なpersistent file treeをNode lifecycleから分離し、backup、restore、verification、retentionを管理します。

## Owned concepts

- `ServerData`
- `BackupRepository`
- `Snapshot`
- `BackupOperation`
- `RestoreOperation`
- `RepositoryCheckOperation`
- `RetentionPolicy`

## Data boundary

Server Dataにはworldだけでなく、server operationに必要なpersistent fileを含めます。

- worlds
- plugins and mods
- configuration
- player and permission data
- server software specific data
- operationに必要なlogsやmetadata

正確なinclude/exclude policyはMinecraft Server distributionと運用要件に応じて定義します。

## Opaque bytes principle

Server Data domainが理解するもの:

- path
- file metadata
- ownership and permission
- size
- repository
- Snapshot
- backup consistency boundary
- retention and verification result

理解しないもの:

- NBTの意味
- player inventoryの意味
- plugin configuration semantics
- game rule
- Minecraft protocol

Minecraft固有のsaveはMinecraft Server domainが実行し、保存可能なconsistency pointを作った後にServer Data backupを開始します。

## Repository model

Accepted direction:

- 永続dataを共有する一つのlogical Minecraft Serverごとに一つのrestic repository
- 一つのrepository内に複数のSnapshotを保持
- backendはCloudflare R2のS3-compatible endpoint
- repository identityとR2 credential scopeをServer Data identityへbindingする
- backup subprocessの終了だけでなく、Snapshotの存在と必要なverificationを確認する
- restic repository passwordは空文字列
- restic password secretは作成・保存しない

restic repository formatは常に暗号化・認証されますが、empty passwordはconfidentiality boundaryではありません。repository objectをreadできる主体はdataを復号できるものとして扱い、R2 credential、bucket/prefix isolation、least privilegeを主要access boundaryとします。

Node Agentのrestic adapterは、empty password repositoryを扱うすべてのcommandへ`--insecure-no-password`を明示的に付与します。Control Plane RPCへraw restic commandやpassword optionを露出させません。

詳細な判断理由とsecurity consequenceは[ADR-0010](../adr/0010-use-restic-on-r2-for-server-data.md)を参照してください。

## Backup result

`restic backup` processが終了code 0を返したことだけを、Server Dataが保護された最終証拠にはしません。

Backup operationは少なくとも次を関連付けます。

- 対象となる`ServerData` identity
- source Nodeとpath
- consistency pointまたはMinecraft save operation
- 作成されたSnapshot ID
- repository identity
- verification policyと結果
- operation timestamps

どのverificationをbackupごとに必須とするか、full data readをどの頻度で行うかはServer Data milestoneで定義します。

## Non-responsibilities

- Minecraft processをいつsave/stopするか
- Workloadをいつ起動するか
- Nodeをいつ削除するか
- file内容のdomain validation
- R2 account全体のidentity management
