# Automated production deployment

`deploy/production_deploy.py` turns the production checkpoint into one
repeatable host-local command. It verifies the immutable GitHub Release, installs
the service and credentials, runs the no-create provider preflight, verifies the
Unix socket and public TLS endpoint, and can run the billable two-generation
acceptance test. Every run writes a secret-free JSON report suitable for sharing
with another operator.

## Account-level inputs

Automation cannot safely invent account ownership decisions. Create these once:

- an AlmaLinux 10 x86-64 control-plane host
- a DNS-only `A`/`AAAA` record whose TCP port 443 reaches that host
- a publicly trusted server certificate for the exact agent hostname
- one dedicated Cloudflare R2 bucket
- a bucket-scoped operator S3 credential used only to initialize restic
- a Cloudflare API token allowed to issue R2 temporary credentials and its
  parent R2 access key ID
- an Akamai API token and one existing enabled firewall
- an SSH public key authorized on temporary nodes

The deploy script never receives the operator R2 S3 credential. Initialize the
acceptance repository from a trusted workstation before deployment:

```bash
export AWS_ACCESS_KEY_ID='operator credential'
export AWS_SECRET_ACCESS_KEY='operator secret'
export AWS_DEFAULT_REGION=auto
export RESTIC_PASSWORD='the same value stored in r2-runtime.env'

restic -r \
  's3:https://ACCOUNT_ID.r2.cloudflarestorage.com/BUCKET/production-acceptance' \
  init
```

## Prepare host-local inputs

Clone the repository on the control-plane host and create a root-only input
directory:

```bash
sudo install -d -m0700 /root/mcserver-production
sudo cp deploy/production-deploy.toml.example \
  /root/mcserver-production/deployment.toml
sudo chmod 0600 /root/mcserver-production/deployment.toml
```

Generate the private client CA once:

```bash
sudo deploy/generate-agent-client-ca.sh \
  /root/mcserver-production/agent-client-ca
```

Place the following input files. Secret inputs must be mode `0600`; validation
rejects broader permissions.

| Input path | Contents |
|---|---|
| `akamai-api-token` | Akamai API token |
| `r2-api-token` | Cloudflare R2 Temporary Credentials API token |
| `remote-tls-private-key.pem` | public endpoint TLS private key |
| `agent-client-ca/agent-client-ca-private-key.pem` | generated private client CA key |
| `r2-runtime.env` | only `RESTIC_PASSWORD` and `AWS_DEFAULT_REGION=auto` |
| `remote-tls-fullchain.pem` | server leaf and intermediate certificates |
| `remote-tls-root-ca.pem` | public trust anchor distributed to nodes |
| `agent-client-ca/agent-client-ca.pem` | generated public client CA certificate |
| `authorized_keys` | SSH public keys for temporary nodes |

The paths can be changed in `deployment.toml`. Set the public hostname, trust
domain, existing Akamai firewall ID, R2 account, parent access key ID, bucket,
and initialized acceptance repository. The example pins both the `v0.1.0`
release commit and the SHA-256 of its checksum manifest.

## One-command production deployment

The following command performs input validation, downloads and verifies the
release archive, installs it, starts with Akamai live creation disabled, passes
the production preflight, enables live creation, and runs the two-generation
acceptance test:

```bash
sudo python3 deploy/production_deploy.py deploy \
  --config /root/mcserver-production/deployment.toml \
  --report /root/mcserver-production/deploy-report.json \
  --go-live \
  --accept-minecraft-eula \
  --confirm-billable-akamai-run \
    I_UNDERSTAND_THIS_CREATES_BILLABLE_AKAMAI_RESOURCES
```

The explicit phrase is a guard against accidentally creating a billable Akamai
VM. It is not an additional manual test. If the live acceptance phase fails,
the script restores `MCSERVER_AKAMAI_LIVE_ENABLED=false` and restarts the
control plane. The acceptance harness also requests normal cleanup unless its
own explicit leave-resources option is used; the deploy command never uses that
option.

To install and stop after the no-create preflight, omit the final four live
arguments:

```bash
sudo python3 deploy/production_deploy.py deploy \
  --config /root/mcserver-production/deployment.toml \
  --report /root/mcserver-production/deploy-report.json
```

## Machine-readable verification

Re-run service, socket, and public TLS verification without reinstalling:

```bash
sudo python3 deploy/production_deploy.py verify \
  --config /root/mcserver-production/deployment.toml \
  --report /root/mcserver-production/verify-report.json
```

A successful report has `"outcome": "passed"` and records each completed phase.
It contains paths, public identifiers, release digests, and status summaries,
but never token or password contents.

Useful failure evidence is:

```bash
sudo cat /root/mcserver-production/deploy-report.json
sudo journalctl -u mcserver-control-plane.service -n 300 --no-pager
```

Review log output before sharing it if a provider unexpectedly included account
data in an error response.
