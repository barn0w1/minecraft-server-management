# Serverの作成と通常運用

## 1. Server定義

ServerごとにTOMLを `/etc/mcserver/servers/` へ置きます。

```bash
sudo install -m0640 -o root -g mcserver \
  /usr/local/share/mcserver/community-server.toml \
  /etc/mcserver/servers/community.toml
sudoedit /etc/mcserver/servers/community.toml
```

`name` は人が操作に使う永久に一意な名前です。1〜63文字の小文字英数字とハイフンを使い、
先頭と末尾は英数字にします。例: `community`、`survival-2026`。

global deployment設定はAkamai resourceのallowlist、同時実行上限、R2 bucket 1つだけを
持ちます。region、type、image、firewall、port、Minecraft設定はServer定義に置きます。

`container_image = "docker.io/itzg/minecraft-server:latest"` は起動時点の最新imageを使います。
完全な再現性が必要になった場合だけdigest pinへ変更します。

## 2. 作成して起動

```bash
sudo mcserverctl \
  --socket /run/mcserver/control-plane.sock \
  server apply \
  --file /etc/mcserver/servers/community.toml \
  --start
```

`apply` はnameでupsertするため、初回は作成、同一内容の再実行はno-opです。

R2 repositoryを手動で指定・初期化する必要はありません。global bucket内の
`servers/community/restic` をcontrol planeが割り当て、最初のnode agentが
`--insecure-no-password` でrepositoryを初期化します。

## 3. 状態確認

```bash
sudo mcserverctl --socket /run/mcserver/control-plane.sock server list
sudo mcserverctl --socket /run/mcserver/control-plane.sock server status community
sudo mcserverctl --socket /run/mcserver/control-plane.sock server instances community
sudo journalctl -u mcserver-control-plane.service -f -o cat
```

`status` にはactive ServerInstance、provider instance ID、public IPv4、agent connectionが
含まれます。Minecraft clientは表示されたpublic IPv4と定義中の `host_port` へ接続します。

## 4. 停止

```bash
sudo mcserverctl \
  --socket /run/mcserver/control-plane.sock \
  server stop community
```

commandの返却はdesired stateのDB commit完了を意味し、snapshotとVM削除の完了ではありません。
`server status community` でactive instanceがなくなり、`current_snapshot_id` が更新される
まで確認します。

正常停止後、Minecraft containerとAkamai VMは削除されます。`/data` のsnapshot、Server、
instance履歴は保持されます。

## 5. 設定変更

停止完了後にTOMLを編集してapplyします。

```bash
sudoedit /etc/mcserver/servers/community.toml
sudo mcserverctl \
  --socket /run/mcserver/control-plane.sock \
  server apply \
  --file /etc/mcserver/servers/community.toml
```

変更できるのはAkamai設定とMinecraft process設定です。name、storage backend、解決済み
repositoryは変更できません。nameを変えたTOMLは別Serverの作成です。厳密な同時更新検出が
必要なclientは `--expected-generation N` を指定します。

## 6. 再起動

```bash
sudo mcserverctl \
  --socket /run/mcserver/control-plane.sock \
  server start community
```

最新published snapshotを新しいVMへrestoreします。public IPv4は世代ごとに変わります。

## 7. Serverを運用対象から外す

まず停止完了を確認し、その後アーカイブします。

```bash
sudo mcserverctl --socket /run/mcserver/control-plane.sock server stop community
sudo mcserverctl --socket /run/mcserver/control-plane.sock server status community
sudo mcserverctl --socket /run/mcserver/control-plane.sock server archive community
```

アーカイブはServer、名前、履歴、snapshot、R2 objectを削除しません。通常一覧とreconcile
対象から外すだけです。R2の `servers/community/` は残り、名前も再利用できません。

```bash
sudo mcserverctl \
  --socket /run/mcserver/control-plane.sock \
  server list --include-archived
```

本システムにはR2 objectを削除するServer APIを設けません。完全削除は別の明示的な
operator作業です。

## 8. control planeの更新

`/etc/mcserver/deployment.toml` のrelease pinを更新し、インストール済みtoolを使います。

```bash
sudo /usr/local/libexec/mcserver/deploy/production_deploy.py check \
  --config /etc/mcserver/deployment.toml \
  --report /var/lib/mcserver-deploy/update-check.json

sudo /usr/local/libexec/mcserver/deploy/production_deploy.py deploy \
  --config /etc/mcserver/deployment.toml \
  --report /var/lib/mcserver-deploy/update.json
```

既存deploymentがliveなら、scriptはno-create preflight後にlive状態を復元します。大きな変更
で2世代acceptanceを再実行する場合だけ `--go-live` と確認optionを付けます。

## 9. 障害時

```bash
sudo systemctl status mcserver-control-plane.service --no-pager
sudo journalctl -u mcserver-control-plane.service -n 300 --no-pager -o cat
sudo mcserverctl --socket /run/mcserver/control-plane.sock server status community
```

VMが残っている場合、Serverをstoppedにしてownership検証付きdeleteを待ちます。
`MCSERVER_AKAMAI_LIVE_ENABLED=false` は新規作成を止めますが、既知VMのstop/deleteは
妨げません。
