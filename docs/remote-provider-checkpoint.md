# Remote Akamai provider checkpoint

This checkpoint proves the remote compute architecture without creating billable resources. The next
operator-controlled boundary is the [production deployment checkpoint](production-deployment-checkpoint.md).

## Prerequisites

The remote provider E2E requires the OpenSSL command-line executable for test certificate generation
and for the control plane's client-certificate authority. Install it before running the checkpoint:

```bash
# Fedora, AlmaLinux, or RHEL
sudo dnf install openssl

# Debian or Ubuntu
sudo apt-get install openssl
```

If OpenSSL is installed outside `PATH`, pass its location explicitly:

```bash
python3 scripts/remote_provider_e2e.py --openssl-binary /path/to/openssl
```

## Acceptance criteria

```bash
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
python3 scripts/deterministic_e2e.py
python3 scripts/remote_provider_e2e.py
```

The remote provider E2E starts the real control plane and node agent, uses real TLS and client
certificate enrollment, and drives the production `reqwest` Akamai adapter against a local fake API.
Only Podman, restic, and the Akamai API are deterministic substitutes. It proves:

- read-only image, region, instance-type, and existing-firewall preflight
- `429 Retry-After` reconciliation delay
- provider-side create success with a lost HTTP response
- adoption by deterministic label instead of duplicate creation
- one-time enrollment with a node-generated P-256 private key and CSR
- client certificate issuance and mTLS reconnect
- invalidation of the bootstrap enrollment token
- runtime secret delivery only after authenticated mTLS registration
- two complete restore/start/stop/snapshot generations
- provider-side deletion with a lost HTTP response
- final absence of managed compute resources

## Provider identity and uncertain responses

Every Akamai-backed `ComputeInstance` uses:

```text
label: mcserver-<ComputeInstance UUID>
tag:   mcserver-managed
tag:   mcserver-scope-<installation scope>
```

Create first searches for the exact label. GET, adoption, firewall verification, and DELETE revalidate
the deterministic label and both ownership tags. A persisted provider ID is never sufficient on its
own. A `404` after an uncertain DELETE is treated as successful convergence.

A VM that disappears before writable data is prepared may be recreated. A VM that disappears after
prepared writable data exists but before result snapshot publication is not recreated from an older
snapshot; reconciliation reports possible writable-data loss.

Startup orphan reaping is disabled by default, requires the live-operations flag, and is limited to the
configured scope. Billable creation is also gated. Deletion of a persisted ownership-verified VM remains
available when the live flag is disabled so an operator can block new charges without blocking cleanup.

## Linode API contract

The adapter uses the Linode API v4 endpoints for images, regions, types, firewalls, Linode instances,
and a Linode's attached firewalls. The create request includes:

```json
{
  "region": "jp-tyo-3",
  "type": "g6-nanode-1",
  "image": "linode/debian13",
  "label": "mcserver-<compute-id>",
  "booted": true,
  "authorized_keys": ["ssh-ed25519 ..."],
  "tags": ["mcserver-managed", "mcserver-scope-production"],
  "firewall_id": 123,
  "metadata": { "user_data": "<base64 cloud-init shell>" }
}
```

Immediately before a create, the adapter checks that the image is available, not deprecated, and
advertises `cloud-init`; that the region advertises `Metadata`; that the instance type exists; that the
configured existing firewall exists and is enabled; and that the installation-scope managed instance
limit is not reached. After create or adoption, it verifies region, image, type, and attached firewall.

HTTP errors retain the bounded Linode `errors` body. `429 Retry-After` participates in per-Server
reconciliation scheduling. Redirects are disabled and production API URLs require HTTPS; only literal
loopback endpoints may use HTTP for tests.

Official references:

- <https://techdocs.akamai.com/linode-api/reference/post-linode-instance>
- <https://techdocs.akamai.com/linode-api/reference/get-image>
- <https://techdocs.akamai.com/linode-api/reference/get-region>
- <https://techdocs.akamai.com/linode-api/reference/get-linode-type>
- <https://techdocs.akamai.com/linode-api/reference/get-firewall>
- <https://techdocs.akamai.com/linode-api/reference/get-linode-firewalls>
- <https://techdocs.akamai.com/linode-api/reference/get-linode-instances>
- <https://techdocs.akamai.com/linode-api/reference/get-linode-instance>
- <https://techdocs.akamai.com/linode-api/reference/delete-linode-instance>
- <https://techdocs.akamai.com/linode-api/reference/filtering-and-sorting>
- <https://techdocs.akamai.com/linode-api/reference/rate-limits>

## Remote-agent trust boundary

The remote listener is separate from the loopback local-agent listener. Connections are always
initiated by the ephemeral node.

The same TLS port supports two deliberately different states:

1. an unauthenticated TLS client may submit only `agent.enroll` with an active one-time token and CSR
2. every normal `agent.register` requires a client certificate signed by the private agent CA

The control plane issues a certificate with a URI SAN of this form:

```text
spiffe://hss-science.org/mcserver/compute/<ComputeInstance UUID>
```

Normal authentication requires all of the following:

- active Akamai `ComputeInstance`
- exact reconnect token
- certificate chain accepted by rustls
- exact leaf certificate DER stored for that ComputeInstance
- unexpired database certificate lifetime

The private key is generated on the node and never crosses the network. The certificate and reconnect
token are persisted atomically with mode `0600`. Enrollment is idempotent for the same CSR, so a lost
response returns the previously recorded certificate and token. A successful mTLS reconnect clears the
enrollment token from SQLite.

## Bootstrap and runtime secrets

Akamai `metadata.user_data` installs `ca-certificates`, `curl`, `openssl`, Podman, and restic; installs
the configured public server trust certificate; downloads one immutable node-agent release asset;
verifies its SHA-256; and installs a hardened root systemd service.

Cloud-init receives no R2 or restic secret. The control plane keeps the Cloudflare API token and
parent R2 access key ID, while its operator-owned runtime file contains only the restic password and
`AWS_DEFAULT_REGION=auto`. After each successful mTLS registration, the control plane issues an
`object-read-write` R2 temporary credential scoped to the exact non-empty repository prefix. The access
key, secret key, and session token are returned only in the registration response, held in node-agent
memory, and added only to restic subprocesses. Long-lived S3 access keys are rejected from the runtime
file.

The remote root service uses host filesystem ownership mode. Local rootless execution continues to use
`podman unshare`; the two ownership boundaries are explicit and are never mixed in one agent process.

## Production profile

```text
control-plane endpoint: agent.mcserver.hss-science.org:443
control-plane OS:       AlmaLinux 10
node image:             linode/debian13
region:                 jp-tyo-3
firewall:               existing operator-managed firewall ID
release target:         x86_64-unknown-linux-musl
```

The production configuration, release workflow, DNS/TLS layout, systemd installation, R2 setup, live
safety gates, and explicit billable E2E are documented in
[production-deployment-checkpoint.md](production-deployment-checkpoint.md).
