# References

外部仕様の理解と実装時の確認に使用するprimary referencesです。versionやprovider behaviorはimplementation時に再確認します。

## RPC and transport

- JSON-RPC 2.0 Specification: <https://www.jsonrpc.org/specification>
- RFC 8259, The JavaScript Object Notation (JSON) Data Interchange Format: <https://www.rfc-editor.org/rfc/rfc8259.html>
- RFC 9113, HTTP/2: <https://www.rfc-editor.org/rfc/rfc9113.html>
- RFC 9110, HTTP Semantics: <https://www.rfc-editor.org/rfc/rfc9110.html>
- RFC 8446, TLS 1.3: <https://www.rfc-editor.org/rfc/rfc8446.html>
- RFC 9525, Service Identity in TLS: <https://www.rfc-editor.org/rfc/rfc9525.html>

## Minecraft runtime

- itzg/docker-minecraft-server source: <https://github.com/itzg/docker-minecraft-server>
- Minecraft Server on Docker documentation: <https://docker-minecraft-server.readthedocs.io/>
- Data directory: <https://docker-minecraft-server.readthedocs.io/en/latest/data-directory/>
- Server properties and RCON password file: <https://docker-minecraft-server.readthedocs.io/en/latest/configuration/server-properties/>
- Environment variables: <https://docker-minecraft-server.readthedocs.io/en/latest/variables/>
- Healthcheck: <https://docker-minecraft-server.readthedocs.io/en/latest/misc/healthcheck/>

## Container runtime

- Podman Quadlet documentation: <https://docs.podman.io/en/latest/markdown/podman-quadlet.1.html>
- Podman systemd unit documentation: <https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html>
- Quadlet basic usage: <https://docs.podman.io/en/latest/markdown/podman-quadlet-basic-usage.7.html>

## Backup and storage

- restic documentation: <https://restic.readthedocs.io/en/stable/>
- restic backup command and exit status: <https://restic.readthedocs.io/en/stable/040_backup.html>
- restic scripting and JSON output: <https://restic.readthedocs.io/en/stable/075_scripting.html>
- restic repository preparation and password options: <https://restic.readthedocs.io/en/stable/030_preparing_a_new_repo.html>
- restic repository design and Snapshot terminology: <https://restic.readthedocs.io/en/stable/100_references.html>
- Cloudflare R2 consistency: <https://developers.cloudflare.com/r2/reference/consistency/>
- Cloudflare R2 S3 API: <https://developers.cloudflare.com/r2/get-started/s3/>
- Cloudflare R2 S3 compatibility: <https://developers.cloudflare.com/r2/api/s3/api/>

## Compute provider

- Linode API v4: <https://techdocs.akamai.com/linode-api/reference/api>
- Create a Linode: <https://techdocs.akamai.com/linode-api/reference/post-linode-instance>
- List Linode types: <https://techdocs.akamai.com/linode-api/reference/get-linode-types>
- Rate limits: <https://techdocs.akamai.com/linode-api/reference/rate-limits>
- Tags and groups: <https://techdocs.akamai.com/cloud-computing/docs/tags-and-groups>
