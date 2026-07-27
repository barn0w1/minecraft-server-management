# クリーンな control-plane host への本番導入

対象は AlmaLinux 10 x86-64 です。以下は repository を `/home/opc/minecraft-server-management`、
機密入力を `/root/mcserver-production` に置く例です。

[外部インフラの前提](production-prerequisites.ja.md) を先に完了してください。

## 1. 基本 package と source

```bash
sudo dnf install -y epel-release
sudo dnf install -y git python3 openssl certbot ca-certificates

git clone https://github.com/barn0w1/minecraft-server-management.git
cd minecraft-server-management
git fetch --tags --force
git checkout v0.2.0
```

実行中の checkout が tag と一致することを確認します。

```bash
git describe --tags --exact-match
git status --short
```

## 2. public TLS certificate

DNS がこの host を向き、TCP 80 を受信できる状態で実行します。

```bash
sudo certbot certonly --standalone \
  -d agent.mcserver.example.org
```

発行後:

```bash
sudo openssl x509 \
  -in /etc/letsencrypt/live/agent.mcserver.example.org/fullchain.pem \
  -noout -subject -issuer -dates

sudo certbot renew --dry-run
```

ACME を Rust daemon 内へ実装しません。証明書の発行と account state は Certbot に任せ、
この repository は renewal 後の安全な反映だけを担当します。

## 3. deployment 入力 directory

```bash
sudo install -d -m0700 /root/mcserver-production

sudo cp deploy/production-deploy.toml.example \
  /root/mcserver-production/deployment.toml
sudo chmod 0600 /root/mcserver-production/deployment.toml

sudo deploy/generate-agent-client-ca.sh \
  /root/mcserver-production/agent-client-ca
```

agent client CA は private mTLS 用です。public Certbot certificate とは別物です。

## 4. secret と public key

次の2ファイルへそれぞれ token だけを1行で保存し、mode `0600` にします。

```text
/root/mcserver-production/akamai-api-token
/root/mcserver-production/r2-api-token
```

対話 editor を使う例:

```bash
sudoedit /root/mcserver-production/akamai-api-token
sudoedit /root/mcserver-production/r2-api-token
sudo chmod 0600 \
  /root/mcserver-production/akamai-api-token \
  /root/mcserver-production/r2-api-token
```

R2 の node runtime file には region だけを置きます。

```bash
printf '%s\n' 'AWS_DEFAULT_REGION=auto' |
  sudo tee /root/mcserver-production/r2-runtime.env >/dev/null
sudo chmod 0600 /root/mcserver-production/r2-runtime.env
```

SSH public key を登録します。

```bash
sudo install -m0640 ~/.ssh/id_ed25519.pub \
  /root/mcserver-production/authorized_keys
```

秘密鍵、長期 S3 secret、restic password はここへ置きません。

## 5. `deployment.toml`

```bash
sudoedit /root/mcserver-production/deployment.toml
```

最低限、次を実環境に合わせます。

- `[release]`: version、`SHA256SUMS` 自体の SHA-256、release commit
- `[service]`: agent hostname、trust domain、Certbot lineage
- `[akamai]`: 利用を許す region/image/type/firewall の配列
- `[r2]`: account ID、parent access key ID、bucket
- `[files]`: Certbot lineage、OS CA bundle、先ほど作成したファイル
- `[acceptance]`: 最初の billable acceptance で使う1組

AlmaLinux の public CA bundle を trust source として指定します。deploy script は Certbot
fullchain を検証できる root CA 1枚だけを自動抽出し、node agent へ埋め込みます。

```toml
[files]
remote_tls_private_key = "/etc/letsencrypt/live/agent.mcserver.example.org/privkey.pem"
remote_tls_fullchain = "/etc/letsencrypt/live/agent.mcserver.example.org/fullchain.pem"
remote_tls_root_ca = "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem"
```

release pin は公開済み asset から取得します。

```bash
curl -fLO \
  https://github.com/barn0w1/minecraft-server-management/releases/download/v0.2.0/SHA256SUMS
sha256sum SHA256SUMS
git rev-parse 'v0.2.0^{commit}'
```

最初の出力を `checksums_sha256`、2つ目を `expected_commit` に設定します。

## 6. 入力と release の検査

```bash
sudo python3 deploy/production_deploy.py check \
  --config /root/mcserver-production/deployment.toml \
  --report /root/mcserver-production/v0.2.0-check-report.json
```

すべて `[passed]` になるまで deploy へ進みません。この command は課金 VM を作りません。

## 7. install と production acceptance

```bash
sudo python3 deploy/production_deploy.py deploy \
  --config /root/mcserver-production/deployment.toml \
  --go-live \
  --accept-minecraft-eula \
  --confirm-billable-akamai-run \
  I_UNDERSTAND_THIS_CREATES_BILLABLE_AKAMAI_RESOURCES \
  --report /root/mcserver-production/v0.2.0-live-report.json
```

script は自動的に次を行います。

1. release checksum と build metadata を検証
2. binary、systemd unit、credential を install
3. live creation 無効のまま DB、PKI、R2、Akamai preflight
4. Unix socket と public TLS を検証
5. live creation を有効化
6. 2世代の VM create、mTLS、Minecraft、stop、snapshot、restore、VM delete
7. secret-free JSON report を保存

acceptance の Server record は監査用に stopped 状態で残ります。Akamai VM は両方とも削除
されていることが成功条件です。

## 8. Certbot renewal

deploy 時に Certbot deploy hook が自動 install されます。hook は certificate/key を
staging、対応検証、control plane restart、`ping` の順に処理し、失敗時は直前の certificate
へ戻します。

deploy hook も含めた dry-run:

```bash
sudo certbot renew --dry-run --run-deploy-hooks

sudo systemctl status certbot-renew.timer --no-pager
sudo systemctl list-timers --all | grep certbot
```

distribution の Certbot package が timer ではなく別の scheduler を使う場合は、
`systemctl list-timers` と package の設定を正本にします。

## 9. 最終確認

```bash
sudo systemctl is-enabled mcserver-control-plane.service
sudo systemctl is-active mcserver-control-plane.service
sudo mcserverctl --socket /run/mcserver/control-plane.sock ping

sudo python3 deploy/production_deploy.py verify \
  --config /root/mcserver-production/deployment.toml \
  --report /root/mcserver-production/verify-report.json
```

次は [Server の作成と通常運用](operations.ja.md) へ進みます。
