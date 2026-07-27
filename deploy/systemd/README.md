# systemd 配置

`mcserver-control-plane.service` は AlmaLinux 10 の専用 `mcserver` user で動作し、TCP 443 の
TLS を直接終端します。reverse proxy は使用しません。

## 配置先

| 種類 | Path |
|---|---|
| deployment config | `/etc/mcserver/deployment.toml` |
| non-secret config | `/etc/mcserver/control-plane.env` |
| public PKI | `/etc/mcserver/pki/` |
| root credential | `/etc/mcserver/credentials/` |
| authorized keys | `/etc/mcserver/authorized_keys` |
| Server definition | `/etc/mcserver/servers/` |
| SQLite | `/var/lib/mcserver/` |
| deployment report | `/var/lib/mcserver-deploy/` |
| Unix socket / temporary PKI | `/run/mcserver/` |
| binaries | `/usr/local/bin/` |
| operator tools | `/usr/local/libexec/mcserver/` |

systemd credential:

- `akamai-api-token`
- `r2-api-token`
- `remote-tls-private-key.pem`
- `agent-client-ca-private-key.pem`
- `r2-runtime.env`

手動配置より [`deploy/production_deploy.py`](../production_deploy.py) を使用してください。
正しい順序は [本番導入手順](../../docs/production-installation.ja.md) にあります。

service hardening は control plane に対して適用されます。一方、ephemeral VM 上の node agent
unit は rootful Podman を動かすため、path 単位の read-only sandbox を使用しません。
