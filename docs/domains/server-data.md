# Server Data domain

## Purpose

Minecraft Serverの完全な永続file treeをNode lifecycleから分離し、backup、restore、verification、retentionを管理します。

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

initial direction:

- persistentなMinecraft Server operation unitごとに一つのrestic repository
- 一つのrepository内に複数のSnapshotを保持
- backendはCloudflare R2のS3-compatible endpoint
- repository identityとcredential scopeをServer Data identityへbindingする
- backup successだけでなくSnapshotの存在と必要なverificationを確認する

restic repositoryは常に暗号化formatを使用します。空passwordを採用する場合も暗号化処理自体は存在しますが、repository read accessを得た主体に対する有効なsecrecy boundaryとはみなしません。主要なaccess boundaryはR2 credential、resource isolation、least privilegeです。

## Non-responsibilities

- Minecraft processをいつsave/stopするか
- Workloadをいつ起動するか
- Nodeをいつ削除するか
- file内容のdomain validation
