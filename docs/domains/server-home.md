# Server Home domain

## Purpose

Minecraft Serverを復元して起動するために必要なpersistent stateを、Node lifecycleから分離して一つのdirectoryとして扱います。

## Directory contract

canonical Node-local path:

```text
/var/lib/mcserver/servers/<server-id>/
├─ manifest.json
├─ secrets/
│  └─ rcon-password
└─ data/
```

### `data/`

itzg/minecraft-server containerの`/data`へmountします。world、player data、plugins、mods、server properties、distribution-specific filesなど、itzg runtimeが保持するpersistent dataを含みます。

### `manifest.json`

そのServer Homeを起動したeffective configurationを持ちます。

minimum content:

```text
schema_version
minecraft_server_id
spec_generation
image_reference
resolved_image_digest
type
version
effective_environment
game_port
resource_settings
written_at
```

secret valueをmanifestへ直接埋め込まず、Server Home内のrelative secret fileを参照します。

### `secrets/`

RCON passwordなど、そのMinecraft Serverのruntimeと一緒にrestoreすべきserver-local secretを保持します。file permissionはrootまたは専用service accountだけに限定します。

Deployment credential、Akamai credential、R2 credential、Agent credentialはServer Homeへ含めません。

## Ownership and placement

- 一つのMinecraftServerは一つのlogical Server Homeを持つ
- 一つのServer Homeは同時に一つのactive Nodeだけでwritableにする
- active allocationのFencing Tokenが一致するAgentだけがmutationできる
- restore先に既存のunknown Server Homeがある場合はoverwriteしない

## Snapshot model

一つのMinecraftServerにつき一つのrestic repositoryを使用し、Server Home root全体をbackupします。

Snapshot metadata:

```text
snapshot_id
minecraft_server_id
spec_generation
source_node_id
operation_id
consistency_mode
started_at
completed_at
```

`consistency_mode`:

- `Offline`: Minecraft Server process停止後のbackup
- `OnlineQuiesced`: RCONでsaveをquiesceしている間のbackup

## Backup success

backup成功条件は次です。

```text
restic backup exits with code 0
AND
structured outputからSnapshot IDを取得できる
```

resticのdocumented contract上、exit code 0は全source fileを含むSnapshotが作成されたことを意味します。追加の`restic snapshots`、`restic check`、full-read verificationをbackup成功条件にしません。

exit code 3はincomplete Snapshotであるため成功扱いしません。

## Online backup

running serverのbackupは次のtyped sequenceを使用します。

```text
1. RCONでsaveを一時停止
2. save-all flushを実行
3. restic backup Server Home root
4. finallyでsaveを再開
5. exit code 0とSnapshot IDをOperation resultへ保存
```

`save.resume`はbackup resultに関係なく試行します。saveを再開できない場合、runtimeはDegradedになりOperatorへ明示します。

## Offline backup

Node release前は次をdefaultにします。

```text
graceful stop
  → process stopped observation
  → restic backup
  → Snapshot result保存
  → Node release
```

Node release gateは「stop後に開始したbackup Operationが成功したこと」です。repository checkはgateに含めません。

## Repository and password

- backendはCloudflare R2
- repository pathはMinecraftServer IDからdeterministicに決める
- すべてのrepositoryでDeployment Restic Passwordを共有する
- passwordはControl Plane databaseではなくdeployment secret fileから読む
- password rotationが必要な場合はrestic key managementを明示的maintenance operationとして扱う

## Retention

retentionはbackupとは別Operationです。initial policyはSnapshot metadataとrestic `forget`/`prune`を使用しますが、backupのたびにpruneしません。

v1 completionに定期`restic check`やautomatic restore drillを要求しません。

## Non-responsibilities

- Minecraft Serverをいつstopするか
- Nodeをいつprovision/deleteするか
- Minecraft file formatのsemantic validation
- Cloudflare R2内部のdurability verification
