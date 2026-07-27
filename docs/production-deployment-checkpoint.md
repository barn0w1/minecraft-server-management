# Production deployment checkpoint

This checkpoint makes the repository ready for an operator-controlled, billable staging deployment.
It does not silently create cloud resources from CI. The final acceptance action remains an explicit
operator command against the installed AlmaLinux 10 control plane.

## Fixed production profile

```text
control-plane OS:        AlmaLinux 10
remote node image:       linode/debian13
Akamai region:           jp-tyo-3
public agent endpoint:   agent.mcserver.hss-science.org:443
Cloudflare DNS mode:     DNS only
TLS termination:         mcserver-control-plane directly
agent steady-state auth: private-CA mTLS plus an exact per-Compute certificate and token
release target:          x86_64-unknown-linux-musl
```

The public endpoint is only the node-agent transport. The client API remains a local Unix socket and
must not be exposed to the Internet. A reverse proxy is not part of the initial deployment. If TCP
port sharing is needed later, use an L4 TLS passthrough proxy so the control plane still authenticates
the client certificate itself.

## Security model

Akamai cloud-init receives a one-time enrollment token, the public server trust certificate, and an
immutable node-agent release URL and SHA-256. It does not receive the private client CA key, Akamai
API token, R2 credentials, or restic password.

On first boot the node agent:

1. generates a P-256 private key locally
2. creates a CSR without exporting the private key
3. performs a server-authenticated TLS enrollment with the one-time token
4. receives a short-lived client certificate and a separate reconnect token
5. persists the key, certificate, and reconnect token mode `0600`
6. reconnects with mTLS
7. causes the control plane to invalidate the enrollment token

The control plane additionally compares the presented leaf certificate DER and reconnect token with
the active `ComputeInstance`. A different certificate signed by the same private CA is not sufficient.
Client certificate validity must exceed the configured maximum VM lifetime by at least one hour. The
reconciler requests a normal stop at the maximum VM lifetime so a connected agent still has time to
publish its final snapshot.

The Akamai live flag gates billable creation and startup orphan reaping. It deliberately does not block
deletion of a persisted, ownership-verified VM. An operator can therefore disable new creation while
still allowing an existing server to stop, snapshot, and remove its VM.

## 1. Accept the source checkpoint

Run the complete secret-free verification before producing a release:

```bash
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
python3 scripts/deterministic_e2e.py
python3 scripts/remote_provider_e2e.py
```

The remote E2E uses the real rustls transport, client certificate enrollment, SQLite repository,
reconciler, and Akamai HTTP adapter against a local fake API. It injects rate limiting, a lost create
response, and a lost delete response without using live credentials.

## 2. Publish an immutable GitHub release

The release workflow is tag-only and refuses a tag that is not annotated, does not match the workspace
version, or does not point at the current `main` commit.

```bash
git switch main
git pull --ff-only
git tag -a v0.1.0 -m 'Release v0.1.0'
git push origin main
git push origin v0.1.0
```

The workflow:

- builds all three binaries for `x86_64-unknown-linux-musl`
- rejects a binary containing a dynamic ELF interpreter
- runs both deterministic E2Es using the release control-plane and node-agent binaries
- starts each binary in official AlmaLinux 10 and Debian 13 containers
- emits deterministic archives, `BUILD-METADATA`, `SHA256SUMS`, and an SPDX 2.3 SBOM
- creates GitHub artifact attestations
- publishes the assets to the annotated GitHub Release

Download the release assets on an operator workstation. Verify the checksums and provenance before
installation:

```bash
sha256sum --check SHA256SUMS
gh attestation verify \
  mcserver-control-plane-v0.1.0-x86_64-unknown-linux-musl \
  --repo barn0w1/minecraft-server-management
```

Use the exact node-agent asset URL and digest in the production environment file. Do not use a branch,
`latest` URL, mutable object, or locally rebuilt binary for cloud-init.

## 3. Prepare DNS and server TLS

Create an `A` and, when applicable, `AAAA` record for:

```text
agent.mcserver.hss-science.org
```

The record must be Cloudflare **DNS only**. It points directly to the AlmaLinux 10 control-plane host.
The existing host firewall must allow TCP 443 to the control plane.

Obtain a publicly trusted server certificate for `agent.mcserver.hss-science.org`. Install:

```text
/etc/mcserver/pki/remote-tls-fullchain.pem
/etc/mcserver/pki/remote-tls-root-ca.pem
/etc/mcserver/credentials/remote-tls-private-key.pem
```

`remote-tls-fullchain.pem` is the leaf followed by required intermediates. `remote-tls-root-ca.pem` is
the trust anchor distributed to remote nodes. The private key remains a root-owned systemd credential.
A certificate renewal hook must atomically replace the installed files and restart
`mcserver-control-plane.service`. Startup preflight rejects a server certificate with less than 24
hours remaining or whose SAN does not match the configured server name.

Before enabling billable operations, confirm the endpoint from an external machine:

```bash
openssl s_client \
  -connect agent.mcserver.hss-science.org:443 \
  -servername agent.mcserver.hss-science.org \
  -CAfile remote-tls-root-ca.pem \
  -verify_return_error </dev/null
```

An anonymous TLS connection is allowed only far enough to submit a valid one-time enrollment RPC.
Normal agent registration requires a valid client certificate.

## 4. Generate the private agent client CA

Generate this CA offline or on the control-plane host before starting the service:

```bash
./deploy/generate-agent-client-ca.sh ./agent-client-ca
```

Install the outputs:

```bash
sudo install -m0644 ./agent-client-ca/agent-client-ca.pem \
  /etc/mcserver/pki/agent-client-ca.pem
sudo install -m0600 ./agent-client-ca/agent-client-ca-private-key.pem \
  /etc/mcserver/credentials/agent-client-ca-private-key.pem
```

Back up the CA key separately from the SQLite database. Loss of this key prevents new node enrollment;
disclosure allows unauthorized certificate issuance and requires CA rotation. Do not use this CA for
server certificates or any unrelated service.

## 5. Initialize R2 and configure temporary credentials

Create a dedicated R2 bucket. Every Server repository must use the account endpoint, configured bucket,
and a non-empty per-repository prefix:

```text
s3:https://<ACCOUNT_ID>.r2.cloudflarestorage.com/<BUCKET>/<SERVER_PREFIX>
```

Create two operator credentials with different purposes:

1. a bucket-scoped S3 credential used only from a trusted workstation to initialize and inspect restic
2. a Cloudflare API token held only by the control plane and permitted to call the R2 Temporary
   Credentials API

The control plane also needs the parent R2 access key ID associated with temporary credential issuance,
but it does not need that parent's S3 secret access key. Initialize each restic prefix once from the
trusted workstation:

```bash
export AWS_ACCESS_KEY_ID='REPLACE_WITH_OPERATOR_S3_ACCESS_KEY'
export AWS_SECRET_ACCESS_KEY='REPLACE_WITH_OPERATOR_S3_SECRET_KEY'
export AWS_DEFAULT_REGION='auto'
export RESTIC_PASSWORD='REPLACE_WITH_A_LONG_RANDOM_PASSWORD'

repository='s3:https://<ACCOUNT_ID>.r2.cloudflarestorage.com/<BUCKET>/minecraft-production'
restic -r "${repository}" init
restic -r "${repository}" snapshots
```

Install the Cloudflare API token as:

```text
/etc/mcserver/credentials/r2-api-token
```

Copy `deploy/systemd/r2-runtime.env.example` to
`/etc/mcserver/credentials/r2-runtime.env` and set only:

```text
RESTIC_PASSWORD=REPLACE
AWS_DEFAULT_REGION=auto
```

Long-lived `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` values are rejected in this file. After each
successful mTLS registration, the control plane requests a fresh `object-read-write` credential scoped
to exactly the Server repository prefix and returns its access key, secret key, and session token only
in the registration response. The node keeps them in memory and applies them only to restic subprocesses.
The default TTL exceeds the maximum VM lifetime plus the shutdown/snapshot safety window; configuration
rejects a TTL longer than seven days or shorter than that safety requirement.

## 6. Install the AlmaLinux 10 service

Install required host tools:

```bash
sudo dnf install -y ca-certificates openssl systemd
```

Extract the verified release archive and install the control plane and CLI:

```bash
sudo ./deploy/install-control-plane.sh \
  ./mcserver-control-plane \
  ./mcserverctl
```

The installer creates the `mcserver` system account and installs the hardened systemd unit, but does
not enable or start it.

Populate these root-owned credentials with mode `0600`:

```text
/etc/mcserver/credentials/akamai-api-token
/etc/mcserver/credentials/r2-api-token
/etc/mcserver/credentials/remote-tls-private-key.pem
/etc/mcserver/credentials/agent-client-ca-private-key.pem
/etc/mcserver/credentials/r2-runtime.env
```

Install the public files:

```text
/etc/mcserver/pki/remote-tls-fullchain.pem
/etc/mcserver/pki/remote-tls-root-ca.pem
/etc/mcserver/pki/agent-client-ca.pem
/etc/mcserver/authorized_keys
```

Edit `/etc/mcserver/control-plane.env` and replace every `REPLACE_...` value. Keep this initial setting:

```text
MCSERVER_AKAMAI_LIVE_ENABLED=false
MCSERVER_AKAMAI_REAP_ORPHANS_ON_START=false
MCSERVER_AKAMAI_MAX_ACTIVE_INSTANCES=1
MCSERVER_AKAMAI_REGION=jp-tyo-3
MCSERVER_AKAMAI_IMAGE=linode/debian13
```

Set the known existing firewall ID and keep the allowlist to the one intended staging type. The
application may attach and inspect that firewall, but it never creates, modifies, or deletes the
firewall itself.

Give the operator account access to the local Unix socket only when needed:

```bash
sudo usermod -aG mcserver "$USER"
```

A new login session is required for the group change.

## 7. Run the no-create production preflight

Start the service while the live flag is false:

```bash
sudo systemctl start mcserver-control-plane.service
sudo systemctl status mcserver-control-plane.service
sudo journalctl -u mcserver-control-plane.service --since today
mcserverctl --socket /run/mcserver/control-plane.sock ping
```

`ExecStartPre` performs SQLite migration and validates:

- server certificate/key match, hostname, and remaining lifetime
- agent client CA certificate/key match and remaining issuance lifetime
- Akamai token and API reachability
- Cloudflare R2 temporary credential issuance for a bounded preflight prefix
- `linode/debian13` availability and `cloud-init` capability
- `jp-tyo-3` and its `Metadata` capability
- every allowed instance type
- the configured existing firewall and enabled status
- the installation-scope managed VM count against the configured maximum

Because the live flag is false, reconciliation cannot create a new Akamai allocation and startup
orphan reaping cannot delete anything.

## 8. Enable one billable staging allocation

After preflight succeeds, set:

```text
MCSERVER_AKAMAI_LIVE_ENABLED=true
```

Restart and verify:

```bash
sudo systemctl restart mcserver-control-plane.service
sudo systemctl status mcserver-control-plane.service
mcserverctl --socket /run/mcserver/control-plane.sock ping
```

Do not enable `MCSERVER_AKAMAI_REAP_ORPHANS_ON_START` for the first live run.

## 9. Run the explicit live two-generation acceptance test

The script cannot run without the exact billable confirmation phrase and explicit EULA acceptance:

```bash
python3 scripts/live_akamai_e2e.py \
  --confirm-billable-akamai-run \
    I_UNDERSTAND_THIS_CREATES_BILLABLE_AKAMAI_RESOURCES \
  --accept-eula \
  --socket /run/mcserver/control-plane.sock \
  --repository \
    's3:https://<ACCOUNT_ID>.r2.cloudflarestorage.com/<BUCKET>/minecraft-production' \
  --firewall-id <EXISTING_FIREWALL_ID> \
  --region jp-tyo-3 \
  --image linode/debian13 \
  --instance-type g6-nanode-1
```

The test must prove:

- one VM at a time
- cloud-init bootstrap and immutable binary checksum verification
- one-time enrollment followed by mTLS reconnect
- fresh prefix-scoped R2 temporary credentials on each successful registration
- public IPv4 observation and Minecraft TCP readiness
- stop, snapshot publication, and VM deletion
- second-generation restore from the first snapshot
- increasing fencing token
- final absence of active `ServerInstance`, `ComputeInstance`, and managed VM

The stopped `Server` row remains for audit. On failure the script requests a normal stop unless
`--leave-resources-on-failure` was explicitly supplied.

## 10. Failure containment and rollback

To block new billable creation without preventing cleanup:

1. set `MCSERVER_AKAMAI_LIVE_ENABLED=false`
2. restart the control plane
3. request every active Server to stop through `mcserverctl`
4. wait for snapshot publication and provider deletion
5. inspect the Akamai account for `mcserver-managed` resources in the configured scope

Do not manually delete a VM that contains the only prepared writable `/data` unless data loss is
accepted. The reconciler intentionally refuses to recreate such a VM from an older snapshot.

Keep startup orphan reaping disabled until the first live test has finished and the observed labels and
tags have been inspected. When enabled, it is restricted to both `mcserver-managed` and the configured
installation scope, but it remains a destructive recovery feature.

## Checkpoint completion

The production deployment checkpoint is complete when:

```text
secret-free CI passes
annotated release publishes static attested artifacts
AlmaLinux 10 service preflight passes with live creation disabled
external DNS and TLS verification passes
live two-generation staging E2E passes
final Akamai managed VM count is zero
R2 contains both published generations under the configured repository prefix
R2 parent S3 secret was never installed on the control plane or a node
service restarts and node-agent mTLS reconnect recovery have been observed
```

The next checkpoint is operational hardening: certificate renewal automation, SQLite backup and restore
drills, Cloudflare API-token rotation, metrics, event audit history, snapshot retention, and disaster recovery.
