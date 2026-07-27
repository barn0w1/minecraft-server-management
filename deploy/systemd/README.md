# AlmaLinux 10 systemd deployment

The production service runs as the dedicated `mcserver` system user and binds the remote node-agent
listener directly on TCP 443 with `CAP_NET_BIND_SERVICE`. No reverse proxy terminates TLS.

The unit uses five systemd credentials:

- `akamai-api-token`
- `r2-api-token`
- `remote-tls-private-key.pem`
- `agent-client-ca-private-key.pem`
- `r2-runtime.env`

Store their source files under `/etc/mcserver/credentials`, owner `root:root`, mode `0600`. Public
certificates and CA certificates belong under `/etc/mcserver/pki`. Non-secret settings belong in
`/etc/mcserver/control-plane.env`.

Install verified release binaries and the unit:

```bash
sudo ./deploy/install-control-plane.sh ./mcserver-control-plane ./mcserverctl
```

Generate the private agent client CA once:

```bash
./deploy/generate-agent-client-ca.sh ./agent-client-ca
```

Populate every required file and replace every `REPLACE_...` value in the installed environment file.
Keep these settings for the first startup:

```text
MCSERVER_AKAMAI_LIVE_ENABLED=false
MCSERVER_AKAMAI_REAP_ORPHANS_ON_START=false
```

Then start and inspect the preflight result:

```bash
sudo systemctl start mcserver-control-plane.service
sudo systemctl status mcserver-control-plane.service
sudo journalctl -u mcserver-control-plane.service --since today
mcserverctl --socket /run/mcserver/control-plane.sock ping
```

`ExecStartPre` validates the database, PKI, scoped R2 temporary credential issuance, Akamai profile,
existing firewall, and managed-instance limit before binding the service. Live-disabled mode blocks new billable creation and startup orphan
reaping, but it does not prevent a known ownership-verified VM from being stopped and deleted.

For a repeatable installation, use
[`deploy/production_deploy.py`](../production_deploy.py) and its
[`configuration example`](../production-deploy.toml.example). The script
performs immutable release verification, installation, no-create preflight,
service and TLS verification, and the optional live two-generation acceptance
test while producing a secret-free JSON report.

The full security boundary and manual reference procedures are in
[`docs/production-deployment-checkpoint.md`](../../docs/production-deployment-checkpoint.md).
