# Server の作成と通常運用

## 1. Server 定義

`examples/community-server.toml` を Server ごとにコピーして管理します。

```bash
sudo install -d -m0750 /etc/mcserver/servers
sudo install -m0640 -o root -g mcserver \
  examples/community-server.toml \
  /etc/mcserver/servers/community.toml
sudoedit /etc/mcserver/servers/community.toml
```

global deployment 設定は利用可能 resource の allowlist と上限だけを持ち、実際に使う
region、type、image、firewall、port、Minecraft 設定は Server 定義に置きます。

`container_image = "docker.io/itzg/minecraft-server:latest"` は起動時点の最新 image を使います。
完全な再現性が必要になった場合だけ digest pin へ変更します。

## 2. 作成して起動

```bash
sudo mcserverctl \
  --socket /run/mcserver/control-plane.sock \
  server apply \
  --file /etc/mcserver/servers/community.toml \
  --start
```

`apply` は name で upsert するため、初回は作成、同一内容の再実行は no-op です。

R2 repository は手動で指定・初期化しません。作成された Server UUID を用いて、
control plane が保存先を決定し、最初の node agent が passwordless repository を作ります。

## 3. 状態確認

```bash
sudo mcserverctl --socket /run/mcserver/control-plane.sock server list
sudo mcserverctl --socket /run/mcserver/control-plane.sock server status SERVER_UUID
sudo mcserverctl --socket /run/mcserver/control-plane.sock server instances SERVER_UUID

sudo journalctl -u mcserver-control-plane.service -f -o cat
```

`status` には active ServerInstance、Akamai provider instance ID、public IPv4、agent connection
が含まれます。Minecraft client は表示された public IPv4 と定義中の `host_port` へ接続します。

## 4. 停止

```bash
sudo mcserverctl \
  --socket /run/mcserver/control-plane.sock \
  server stop SERVER_UUID
```

command の返却は desired state の DB commit 完了を意味し、snapshot と VM delete の完了では
ありません。`server status` で active instance がなくなり、`current_snapshot_id` が更新
されるまで確認します。

正常停止後:

- Minecraft container は削除
- `/data` は R2 snapshot として公開
- Akamai VM は削除
- Server と履歴は SQLite に保持

## 5. 設定変更

必ず停止完了後に TOML を編集し、再度 apply します。

```bash
sudoedit /etc/mcserver/servers/community.toml

sudo mcserverctl \
  --socket /run/mcserver/control-plane.sock \
  server apply \
  --file /etc/mcserver/servers/community.toml
```

同時更新を厳密に検出したい client は `--expected-generation N` を付けます。

変更できるもの:

- Akamai region、type、image、firewall
- Minecraft image、type、version、port、環境変数、stop timeout

変更できないもの:

- Server name
- storage backend
- repository

name を変えた TOML は別 Server の作成として扱われます。

## 6. 再起動

```bash
sudo mcserverctl \
  --socket /run/mcserver/control-plane.sock \
  server start SERVER_UUID
```

最新 published snapshot を新しい VM へ restore します。Akamai VM の public IPv4 は世代ごとに
変わるため、固定接続先が必要なら将来 client/bot 側で DNS 更新を実装します。

## 7. control plane の更新

新しい release の `deployment.toml` pin を更新し、同じ deploy script を再実行します。
通常の version 更新で billable acceptance を毎回行わない場合:

```bash
sudo python3 deploy/production_deploy.py check \
  --config /root/mcserver-production/deployment.toml \
  --report /root/mcserver-production/update-check-report.json

sudo python3 deploy/production_deploy.py deploy \
  --config /root/mcserver-production/deployment.toml \
  --report /root/mcserver-production/update-report.json
```

既存 deployment が live なら、script は一度 live creation を止めて no-create preflight を
行い、検証後に元の live 状態を復元します。初回導入または既に live=false の環境を、この
command が暗黙に live にすることはありません。大きな変更後に2世代 acceptance も再実行
したい場合だけ `--go-live` と確認 option を付けます。

## 8. 障害時に見るもの

```bash
sudo systemctl status mcserver-control-plane.service --no-pager
sudo journalctl -u mcserver-control-plane.service -n 300 --no-pager -o cat
sudo mcserverctl --socket /run/mcserver/control-plane.sock server status SERVER_UUID
```

VM が残っている場合、先に Server を stopped にして reconciler の ownership-verified delete
を待ちます。緊急時に provider console から手動削除した場合は、その事実を確認してから
control plane を再起動します。`MCSERVER_AKAMAI_LIVE_ENABLED=false` は新規作成を止めますが、
既知 VM の stop/delete は妨げません。
