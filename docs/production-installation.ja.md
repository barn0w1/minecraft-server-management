# クリーンなcontrol-plane hostへの本番導入

対象はAlmaLinux 10 x86-64です。設定は `/etc`、永続状態は `/var/lib`、実行時状態は
`/run` に置き、daemonはlogin不可の専用 `mcserver` userで動かします。`/root` や
operatorのhome directoryは運用データの保存先にしません。

[外部インフラの前提](production-prerequisites.ja.md) を先に完了してください。

## 1. packageとrelease source

```bash
sudo dnf install -y epel-release
sudo dnf install -y git python3 openssl certbot ca-certificates

git clone https://github.com/barn0w1/minecraft-server-management.git
cd minecraft-server-management
git fetch --tags --force
git checkout v0.3.0
git describe --tags --exact-match
git status --short
```

最後の出力は空である必要があります。source checkoutはdeploy実行時だけ使います。
インストール後のdaemonはcheckoutに依存しません。

## 2. public TLS certificate

agent用DNSがこのhostを向き、TCP 80を受信できる状態で発行します。

```bash
sudo certbot certonly --standalone \
  -d agent.mcserver.example.org

sudo openssl x509 \
  -in /etc/letsencrypt/live/agent.mcserver.example.org/fullchain.pem \
  -noout -subject -issuer -dates

sudo certbot renew --dry-run
```

ACME account、発行、renewalはCertbotに任せます。Rust daemonは証明書を発行せず、
repositoryのdeploy hookがrenewal後の対応検証、反映、異常時rollbackを担当します。

## 3. 標準directoryを準備

```bash
sudo deploy/prepare-control-plane-host.sh
```

このcommandは専用user、directory、passwordless restic用environment、private agent CAを
一度だけ作ります。既存の秘密情報は上書きしません。

| 種類 | Path |
|---|---|
| deployment pinとglobal設定 | `/etc/mcserver/deployment.toml` |
| root credential | `/etc/mcserver/credentials/` |
| public PKI | `/etc/mcserver/pki/` |
| Server定義 | `/etc/mcserver/servers/` |
| SQLite | `/var/lib/mcserver/control-plane.db` |
| secret-free deploy report | `/var/lib/mcserver-deploy/` |
| socketと一時PKI | `/run/mcserver/` |
| binary | `/usr/local/bin/` |

## 4. credentialとSSH公開鍵

次のファイルへtokenだけを1行で保存します。

```bash
sudoedit /etc/mcserver/credentials/akamai-api-token
sudoedit /etc/mcserver/credentials/r2-api-token
sudo chmod 0600 \
  /etc/mcserver/credentials/akamai-api-token \
  /etc/mcserver/credentials/r2-api-token
```

operatorのSSH公開鍵を登録します。

```bash
sudo install -m0640 -o root -g mcserver \
  ~/.ssh/id_ed25519.pub \
  /etc/mcserver/authorized_keys
```

秘密SSH鍵、長期S3 secret、restic passwordは配置しません。
`/etc/mcserver/credentials/r2-runtime.env` は
`AWS_DEFAULT_REGION=auto` だけを含む状態のまま使います。

## 5. deployment.toml

```bash
sudoedit /etc/mcserver/deployment.toml
```

次を実環境に合わせます。

- `[release]`: v0.3.0、`SHA256SUMS`自体のSHA-256、release commit
- `[service]`: agent hostname、trust domain、Certbot lineage
- `[akamai]`: 許可するregion/image/type/firewallと同時実行上限
- `[r2]`: account ID、parent access key ID、control plane専用bucket 1つ
- `[files]`: 通常は標準pathのまま。Certbot hostnameだけ変更
- `[acceptance]`: 初回のbillable acceptanceで使う1組

release pinは公開assetから取得します。

```bash
curl -fLO \
  https://github.com/barn0w1/minecraft-server-management/releases/download/v0.3.0/SHA256SUMS
sha256sum SHA256SUMS
git rev-parse 'v0.3.0^{commit}'
```

最初の出力を `checksums_sha256`、2つ目を `expected_commit` に設定します。

## 6. 課金なしの入力検査

```bash
sudo python3 deploy/production_deploy.py check \
  --config /etc/mcserver/deployment.toml \
  --report /var/lib/mcserver-deploy/v0.3.0-check.json
```

すべて `[passed]` になるまでdeployへ進みません。このcommandはVMを作成しません。

## 7. installと本番acceptance

```bash
sudo python3 deploy/production_deploy.py deploy \
  --config /etc/mcserver/deployment.toml \
  --go-live \
  --accept-minecraft-eula \
  --confirm-billable-akamai-run \
  I_UNDERSTAND_THIS_CREATES_BILLABLE_AKAMAI_RESOURCES \
  --report /var/lib/mcserver-deploy/v0.3.0-live.json
```

scriptは次を自動実行します。

1. release checksum、build metadata、node-agent digestを検証
2. binary、operator tool、systemd unit、credentialを標準pathへinstall
3. live creation無効でDB、PKI、R2、Akamai preflight
4. Unix socketとpublic TLSを検証
5. live creationを有効化
6. 2世代のVM create、mTLS、Minecraft、stop、snapshot、restore、VM delete
7. acceptance Serverをアーカイブし、R2 repositoryを保持
8. secret-free JSON reportを保存

成功条件には、両方のAkamai VMが削除済みであることが含まれます。

## 8. Certbot renewal

deploy時にCertbot deploy hookがinstallされます。

```bash
sudo certbot renew --dry-run --run-deploy-hooks
sudo systemctl status certbot-renew.timer --no-pager
sudo systemctl list-timers --all | grep certbot
```

distributionが別schedulerを使う場合は、そのpackage設定を正本にします。

## 9. 最終確認

```bash
sudo systemctl is-enabled mcserver-control-plane.service
sudo systemctl is-active mcserver-control-plane.service
sudo mcserverctl --socket /run/mcserver/control-plane.sock ping

sudo /usr/local/libexec/mcserver/deploy/production_deploy.py verify \
  --config /etc/mcserver/deployment.toml \
  --report /var/lib/mcserver-deploy/verify.json
```

次は [Serverの作成と通常運用](operations.ja.md) へ進みます。
