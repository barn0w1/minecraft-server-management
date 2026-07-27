# Client API

operator CLI や将来の Discord bot は、同じ JSON-RPC 2.0 client API を使用します。

- transport: Unix stream socket
- production path: `/run/mcserver/control-plane.sock`
- framing: UTF-8 JSON 1件 + `\n`
- maximum frame: control-plane 設定に従う
- request ID: string または number

## Method

| Method | Params | Result |
|---|---|---|
| `system.ping` | `null` | status、version |
| `server.create` | name、desired spec | Server |
| `server.apply` | name、desired spec、optional generation | Server |
| `server.get` | server UUID | Server |
| `server.list` | `null` | Server list |
| `server.status` | server UUID | Server + active instance/compute |
| `server.set_desired_state` | UUID、state、optional generation | Server |
| `server_instance.get` | instance UUID | ServerInstance |
| `server_instance.list` | server UUID | history |

`server.apply` は name で upsert します。既存 Server の設定変更は stopped 時のみ可能です。
`expected_generation` が一致しなければ conflict になります。

## Server 作成 request

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

response の `spec.data` は解決済み repository を含みます。

```json
{
  "backend": "r2_restic",
  "repository": "s3:https://ACCOUNT.r2.cloudflarestorage.com/BUCKET/servers/SERVER_UUID/restic"
}
```

`local_restic` はローカル検証用で、request に `repository` が必要です。

## CLI

```text
mcserverctl [--socket PATH] ping
mcserverctl [--socket PATH] server list
mcserverctl [--socket PATH] server get SERVER_ID
mcserverctl [--socket PATH] server status SERVER_ID
mcserverctl [--socket PATH] server instances SERVER_ID
mcserverctl [--socket PATH] server start SERVER_ID
mcserverctl [--socket PATH] server stop SERVER_ID
mcserverctl [--socket PATH] server create --file FILE [--start]
mcserverctl [--socket PATH] server apply --file FILE [--start]
  [--expected-generation GENERATION]
```

通常は idempotent な `server apply --file` を使います。TOML schema は
[`examples/community-server.toml`](../examples/community-server.toml) を参照してください。

## 状態変更の意味

`server start` / `stop` の成功は desired state と generation の DB commit 完了を表します。
外部 VM、Minecraft、snapshot、delete の完了を同期的には待ちません。`server.status` を
poll して収束を確認します。

## Error

JSON-RPC 標準 error に加え、validation、not found、generation conflict、repository/provider
failure を message に含めます。client は timeout 後の mutation を未実行と決めつけず、
`server.get` / `server.status` で再観測してください。
