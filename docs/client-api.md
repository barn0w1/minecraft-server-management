# Client API

operator CLI と将来の Discord bot は、同じ JSON-RPC 2.0 API を使用します。

- transport: Unix stream socket
- production path: `/run/mcserver/control-plane.sock`
- framing: UTF-8 JSON 1件と改行
- request ID: string または number

## Server name

operator向けのServer操作はUUIDではなく `server_name` を使います。名前は次をすべて満たす
必要があります。

- 1〜63文字
- 小文字ASCII英字、数字、ハイフンのみ
- 先頭と末尾は英字または数字
- control plane内で永久に一意

例: `community`、`survival-2026`。アーカイブ済みの名前も再利用できません。responseには
内部参照と監査のためUUIDも含まれます。

## Method

| Method | Params | Result |
|---|---|---|
| `system.ping` | `null` | status、version |
| `server.create` | name、desired spec | Server |
| `server.apply` | name、desired spec、optional generation | Server |
| `server.get` | server_name | Server |
| `server.list` | optional include_archived | Server list |
| `server.status` | server_name | Server + active instance/compute |
| `server.set_desired_state` | server_name、state、optional generation | Server |
| `server.archive` | server_name、optional generation | Server |
| `server_instance.get` | instance UUID | ServerInstance |
| `server_instance.list` | server_name | history |

`server.apply` はnameでupsertします。既存Serverの設定変更はstopped時のみ可能です。
`expected_generation` が一致しなければconflictです。

`server.archive` は、Serverがstoppedでactive instanceがない場合だけ成功します。行、名前、
履歴、snapshot、R2 repositoryは削除せず、defaultの `server.list` とreconcile対象から
外します。監査表示は `{"include_archived":true}` を指定します。

## Server作成request

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "server.apply",
  "params": {
    "name": "community",
    "spec": {
      "compute": {
        "provider": "akamai",
        "region": "jp-tyo-3",
        "instance_type": "g6-nanode-1",
        "image": "linode/debian13",
        "firewall_id": 123456
      },
      "process": {
        "container_image": "docker.io/itzg/minecraft-server:latest",
        "server_type": "VANILLA",
        "version": "LATEST",
        "host_port": 25565,
        "stop_timeout_seconds": 120,
        "accept_eula": true,
        "environment": {
          "MEMORY": "768M"
        }
      },
      "data": {
        "backend": "r2_restic"
      }
    }
  }
}
```

responseの `spec.data` には、global bucketと名前から解決したrepositoryが含まれます。

```json
{
  "backend": "r2_restic",
  "repository": "s3:https://ACCOUNT.r2.cloudflarestorage.com/BUCKET/servers/community/restic"
}
```

`local_restic` はローカル検証専用で、requestに `repository` が必要です。

## CLI

```text
mcserverctl [--socket PATH] ping
mcserverctl [--socket PATH] server list [--include-archived]
mcserverctl [--socket PATH] server get SERVER_NAME
mcserverctl [--socket PATH] server status SERVER_NAME
mcserverctl [--socket PATH] server instances SERVER_NAME
mcserverctl [--socket PATH] server start SERVER_NAME
mcserverctl [--socket PATH] server stop SERVER_NAME
mcserverctl [--socket PATH] server archive SERVER_NAME
mcserverctl [--socket PATH] server create --file FILE [--start]
mcserverctl [--socket PATH] server apply --file FILE [--start]
  [--expected-generation GENERATION]
```

通常はidempotentな `server apply --file` を使います。TOML schemaは
[`examples/community-server.toml`](../examples/community-server.toml) を参照してください。

## 状態変更の意味

`server start` / `stop` の成功はdesired stateとgenerationのDB commit完了を表します。
外部VM、Minecraft、snapshot、deleteの完了を同期的には待ちません。`server.status` を
pollして収束を確認します。

JSON-RPC標準errorに加え、validation、not found、generation conflict、active instance、
repository/provider failureをmessageに含めます。timeout後のmutationを未実行と決めつけず、
`server.get` / `server.status` で再観測してください。
