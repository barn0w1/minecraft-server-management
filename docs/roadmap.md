# Roadmap

## Completed: local vertical slice

- durable Server desired state and optimistic generation checks
- reconciler-owned ServerInstance and ComputeInstance with active uniqueness
- fencing-token-protected restic snapshot publication
- loopback node-agent registration, reconnect, and durable operation state
- rootless Podman Minecraft lifecycle with subordinate-ID-safe data operations
- real two-generation Fedora/Podman/restic/Minecraft E2E

## Completed: pre-cloud reliability checkpoint

- managed-resource labels and installation-local scope
- startup orphan container, process, and state-directory reaping
- TCP port preflight and label-scoped failed-test cleanup
- deterministic fake Podman and restic command adapters
- process-level two-generation E2E using the real daemons
- injected transient Podman and restic failure recovery
- reusable Unix-socket JSON-RPC client
- `mcserverctl` operator commands
- aggregate read-only `server.status`

See [the pre-cloud checkpoint definition](pre-cloud-checkpoint.md).

## Completed: remote Akamai provider checkpoint

- provider-neutral compute dispatch preserving the local provider
- real Akamai/Linode API v4 create, list, get, firewall-observation, and delete adapter
- image, region, instance-type, and existing-firewall preflight
- deterministic labels plus managed and installation-scope tags
- adoption after uncertain create responses and convergence after uncertain delete responses
- structured API errors, pagination, request timeouts, and `Retry-After`
- direct remote TLS listener and node-generated P-256 private key
- one-time enrollment, CSR signing, and exact per-Compute mTLS authentication
- cloud-init installation with immutable HTTPS release and SHA-256 verification
- post-mTLS issuance and in-memory delivery of prefix-scoped R2 temporary credentials
- systemd supervision of the remote root node agent
- deterministic remote E2E using the real daemons, rustls transport, and reqwest adapter

See [the remote provider checkpoint definition](remote-provider-checkpoint.md).

## Implemented: production deployment checkpoint

- fixed AlmaLinux 10 control-plane and Debian 13 `jp-tyo-3` node profile
- Cloudflare DNS-only endpoint at `agent.mcserver.hss-science.org:443`
- direct control-plane TLS termination with private client CA mTLS
- PKI preflight for key matching, server name, remaining validity, and issuance lifetime
- immutable existing firewall ID and provider specification allowlists
- explicit live-create gate, one-instance limit, and maximum VM lifetime
- deletion remains available after live creation is disabled
- AlmaLinux systemd unit using credentials, dedicated user, and service hardening
- private agent CA generation and installation helpers
- Cloudflare R2 Temporary Credentials API with per-repository prefix scope
- explicit billable two-generation live production harness with cleanup
- secret-free pinned GitHub CI
- tagged static musl release build with release-binary E2Es
- AlmaLinux 10 and Debian 13 release smoke tests
- deterministic archive, SHA-256 manifest, SPDX SBOM, and GitHub artifact attestation
- Dependabot configuration and repository security policy

See [the production deployment checkpoint](production-deployment-checkpoint.md).

The repository is ready for operator validation. Completion of the live checkpoint requires the
operator-owned Akamai token, existing firewall ID, DNS record, public server certificate, private
agent CA, initialized R2 restic repository, and an explicitly confirmed billable run.

## Next checkpoint: live production evidence and operations

1. pass workspace checks and both deterministic E2Es on the final commit
2. publish and verify an annotated attested GitHub Release
3. install the control plane on AlmaLinux 10 with live creation disabled
4. pass PKI, Akamai, DNS, and external TLS preflight
5. run the explicit two-generation Akamai production E2E
6. restart the control plane while a VM is active and verify mTLS reconnect without duplicate creation
7. rotate the Cloudflare API token, restic password, and agent client CA in documented drills
8. automate SQLite online backup, restore verification, and off-host retention
9. record billing, provider inventory, logs, and R2 snapshots as live-checkpoint evidence
10. enable scoped startup orphan reaping only after its ownership labels have been inspected

## Following checkpoints

- automated public server certificate renewal and controlled service restart
- metrics, structured event audit history, and alerting
- snapshot listing, explicit rollback, retention, prune, and repository integrity checks
- disaster-recovery runbooks and periodic restore exercises
- optional Quadlet execution after the Debian 13 target behavior is proven
- Unix-socket authorization and an authenticated remote client gateway
- Discord bot or web UI built on the same client API
- multi-control-plane storage and leadership only when availability requirements justify it

Continue to prefer complete vertical capabilities over generic frameworks. Add resources only for
independent lifecycles, preserve `/data` opacity, and extract provider traits from proven concrete
implementations rather than speculative abstractions.
