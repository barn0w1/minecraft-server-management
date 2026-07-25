# References

外部仕様の理解と実装時の確認に使用するprimary referencesです。versionやprovider behaviorは実装時に再確認します。

## Protocols

- JSON-RPC 2.0 Specification: <https://www.jsonrpc.org/specification>
- RFC 8259, The JavaScript Object Notation (JSON) Data Interchange Format: <https://www.rfc-editor.org/rfc/rfc8259.html>
- RFC 9000, QUIC: A UDP-Based Multiplexed and Secure Transport: <https://www.rfc-editor.org/rfc/rfc9000.html>
- RFC 9001, Using TLS to Secure QUIC: <https://www.rfc-editor.org/rfc/rfc9001.html>
- RFC 8446, TLS 1.3: <https://www.rfc-editor.org/rfc/rfc8446.html>
- RFC 9525, Service Identity in TLS: <https://www.rfc-editor.org/rfc/rfc9525.html>
- Quinn documentation: <https://docs.rs/quinn/latest/quinn/>

## Minecraft

- Minecraft Java Edition 1.21.9, Minecraft Server Management Protocol introduction: <https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-9>

Minecraft Server Management Protocolは更新されるため、supported Minecraft versionごとにprotocol versionと`rpc.discover`結果を実装時に確認します。

## Workload runtime

- Podman Quadlet documentation: <https://docs.podman.io/en/latest/markdown/podman-quadlet.1.html>
- Podman systemd unit documentation: <https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html>
- Quadlet basic usage: <https://docs.podman.io/en/latest/markdown/podman-quadlet-basic-usage.7.html>

## Server Data

- restic documentation: <https://restic.readthedocs.io/en/stable/>
- restic repositories with empty password: <https://restic.readthedocs.io/en/stable/030_preparing_a_new_repo.html#repositories-with-empty-password>
- restic repository design and terminology: <https://restic.readthedocs.io/en/stable/100_references.html>
- Cloudflare R2 S3 API: <https://developers.cloudflare.com/r2/get-started/s3/>
- Cloudflare R2 S3 compatibility: <https://developers.cloudflare.com/r2/api/s3/api/>

## Compute provider

- Linode API v4: <https://techdocs.akamai.com/linode-api/reference/api>
- Create a Linode: <https://techdocs.akamai.com/linode-api/reference/post-linode-instance>
- List Linode types: <https://techdocs.akamai.com/linode-api/reference/get-linode-types>
