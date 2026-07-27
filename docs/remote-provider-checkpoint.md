# Remote Akamai provider checkpoint

This checkpoint is the deployable boundary immediately before a billable Akamai Cloud run. It adds the real Linode API client and the real remote-agent transport while keeping the acceptance test deterministic and free of cloud charges.

## Acceptance criteria

The checkpoint is accepted when all of the following pass:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
python3 scripts/deterministic_e2e.py
python3 scripts/remote_provider_e2e.py
```

The remote provider E2E starts the real control-plane and node-agent binaries. It uses a real TLS connection and the production `reqwest` Akamai adapter against a local fake Linode API, while substituting deterministic fake Podman and restic commands. It must prove:

- startup deletion of a scoped orphan Akamai instance
- image `cloud-init` and region `Metadata` capability preflight before VM creation
- a create request that succeeds provider-side but loses its response
- adoption by deterministic provider label instead of duplicate creation
- TLS server-name and CA validation
- one-time bootstrap enrollment and replacement reconnect-token persistence
- invalidation of the bootstrap enrollment token after reconnect
- two complete generations with snapshot restore and fencing-token increase
- provider deletion and remote-agent shutdown after each generation

A live run is intentionally not part of CI because it requires a billable account, a real API token, public DNS/TLS, firewall policy, and object-storage credentials.

## Provider identity and uncertain responses

Every Akamai-backed `ComputeInstance` gets:

```text
label: mcserver-<ComputeInstance UUID>
tag:   mcserver-managed
tag:   mcserver-scope-<installation scope>
```

Before creating a VM, the provider searches for the exact label. A timed-out or failed create response is therefore reconciled by discovery instead of issuing another create. The persisted provider ID is never sufficient by itself: GET and DELETE paths also verify the deterministic label and both ownership tags before controlling the VM.

A `404` from GET or DELETE means the provider resource is absent. If the VM disappears before writable data is prepared, it may be recreated. If prepared writable data exists and no result snapshot has been published, recreation from an older snapshot is refused and reconciliation reports possible data loss.

The optional startup reaper lists managed instances using `X-Filter`, paginates with a page size of 500, filters the installation scope again client-side, adopts a matching active database allocation, and deletes only scoped orphans. It is disabled by default because deletion is destructive.

## Linode API contract

The adapter uses the Akamai Cloud Computing Linode API v4:

- `GET /v4/images/{imageId}`
- `GET /v4/regions/{regionId}`
- `POST /v4/linode/instances`
- `GET /v4/linode/instances`
- `GET /v4/linode/instances/{id}`
- `DELETE /v4/linode/instances/{id}`

The create payload includes:

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

API errors preserve the bounded Linode `errors` array. HTTP `429` reads `Retry-After` seconds and feeds that delay into per-Server reconciliation backoff. The client uses bearer authentication, JSON, a bounded request timeout, and HTTPS except for loopback-only tests.

Immediately before a billable create request, the adapter verifies that the selected image is available, is not deprecated, and advertises the exact `cloud-init` capability. It also verifies that the selected region advertises the exact `Metadata` capability. A failed preflight prevents VM creation. The initial production target is `linode/debian13` in `jp-tyo-3`; the instance type and firewall remain explicit Server configuration rather than hidden provider defaults.

Official references:

- [Create a Linode](https://techdocs.akamai.com/linode-api/reference/post-linode-instance)
- [Get an image](https://techdocs.akamai.com/linode-api/reference/get-image)
- [Get a region](https://techdocs.akamai.com/linode-api/reference/get-region)
- [List Linodes](https://techdocs.akamai.com/linode-api/reference/get-linode-instances)
- [Get a Linode](https://techdocs.akamai.com/linode-api/reference/get-linode-instance)
- [Delete a Linode](https://techdocs.akamai.com/linode-api/reference/delete-linode-instance)
- [Filtering and sorting](https://techdocs.akamai.com/linode-api/reference/filtering-and-sorting)
- [Pagination](https://techdocs.akamai.com/linode-api/reference/pagination)
- [Rate limits](https://techdocs.akamai.com/linode-api/reference/rate-limits)

## Remote-agent trust boundary

The remote listener is separate from the loopback listener and requires TLS. The node agent initiates the connection; no inbound agent RPC port is opened on the ephemeral VM.

The control plane presents a certificate whose name matches `MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_SERVER_NAME`. The VM receives only the configured CA certificate and validates the server through rustls.

Each Akamai allocation has two independent random secrets:

1. an enrollment token embedded in cloud-init
2. a reconnect token retained only by the control-plane database

The first TLS registration can use the enrollment token. The control plane returns the stable reconnect token and closes that enrollment session. The node agent writes the replacement token atomically as mode `0600`, then reconnects immediately. Registration with the reconnect token clears the enrollment token from SQLite. If the first response is lost, the still-valid enrollment token returns the same reconnect token, so enrollment remains idempotent.

Both secrets are ComputeInstance-bound. Registration verifies the active allocation and expected provider. Terminating the ComputeInstance revokes both because inactive rows cannot authenticate. Configuration `Debug` implementations redact API and agent secrets.

This checkpoint uses server-authenticated TLS plus high-entropy per-allocation bearer credentials. Mutual TLS and external secret-manager integration remain optional production hardening, not prerequisites for the first live vertical-slice validation.

## Bootstrap and node supervision

Akamai `metadata.user_data` contains a Base64-encoded shell bootstrap that:

1. installs CA certificates, curl, Podman, and restic with `dnf` or `apt-get`
2. installs the configured control-plane CA
3. downloads the exact node-agent binary over HTTPS
4. verifies its configured SHA-256 before installation
5. writes a mode-`0600` environment file
6. installs and starts a systemd service for the node agent

The operator-supplied environment file may contain restic and object-storage credentials, but it may not override `MCSERVER_NODE_AGENT_*` keys. The Server's `data.repository` remains the authoritative opaque restic repository address.

The remote node currently uses the same proven direct-Podman executor as the local vertical slice, supervised indirectly by the systemd-managed node agent. Data access is explicit: local unprivileged agents default to `podman_user_namespace`, while the root systemd service generated for remote VMs uses `host`. Restic, restore directory swaps, and cleanup therefore run in the same ownership boundary that created the data. Quadlet is deferred until the first real target image is validated because its rootless/rootful paths and unit-generation behavior are image- and systemd-version-sensitive.

## Required control-plane configuration

Remote TLS settings are all-or-none:

```bash
export MCSERVER_CONTROL_PLANE_REMOTE_AGENT_LISTEN_ADDRESS='0.0.0.0:39002'
export MCSERVER_CONTROL_PLANE_REMOTE_AGENT_PUBLIC_ADDRESS='control.example.com:39002'
export MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_SERVER_NAME='control.example.com'
export MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CERTIFICATE='/etc/mcserver/tls/server.crt'
export MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_PRIVATE_KEY='/etc/mcserver/tls/server.key'
export MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CA_CERTIFICATE='/etc/mcserver/tls/ca.crt'
export MCSERVER_CONTROL_PLANE_NODE_AGENT_DOWNLOAD_URL='https://downloads.example.com/mcserver-node-agent'
export MCSERVER_CONTROL_PLANE_NODE_AGENT_SHA256='<64 lowercase hexadecimal characters>'
```

Akamai settings:

```bash
export MCSERVER_AKAMAI_API_TOKEN='<secret>'
export MCSERVER_AKAMAI_API_BASE_URL='https://api.linode.com/v4'
export MCSERVER_AKAMAI_AUTHORIZED_KEYS_FILE='/etc/mcserver/authorized_keys'
export MCSERVER_AKAMAI_NODE_AGENT_ENVIRONMENT_FILE='/etc/mcserver/node-agent.env'
export MCSERVER_AKAMAI_SCOPE='production'
export MCSERVER_AKAMAI_REQUEST_TIMEOUT_SECONDS='30'
export MCSERVER_AKAMAI_REAP_ORPHANS_ON_START='false'
```

The API token needs Linode read/write access. Keep it in the control-plane service credential environment or secret store; it is never sent to a node agent.

Example node-agent environment for an R2-compatible restic repository:

```text
RESTIC_PASSWORD_FILE=/etc/mcserver/restic-password
AWS_ACCESS_KEY_ID=<R2 access key>
AWS_SECRET_ACCESS_KEY=<R2 secret key>
AWS_DEFAULT_REGION=auto
```

The Server's repository can then be an S3-compatible restic URL such as `s3:https://<account-id>.r2.cloudflarestorage.com/<bucket>/<server-prefix>`.

## Network and live-run handoff

Before a live run, the operator must provide:

- public DNS resolving the remote-agent TLS name to the control-plane VM
- TCP access from ephemeral VMs to the remote-agent listener
- a certificate chain trusted by the configured CA
- an Akamai firewall that permits the Minecraft port and required outbound traffic
- optional SSH access through the supplied public keys
- an HTTPS location for the statically linked or otherwise target-compatible node-agent binary
- an initialized restic repository and least-privilege R2 credentials

The first production compatibility profile is:

```text
image:  linode/debian13
region: jp-tyo-3
```

The first live run should use a dedicated scope, the smallest suitable instance type, a short manual observation window, and explicit desired-state stop before enabling startup orphan reaping.
