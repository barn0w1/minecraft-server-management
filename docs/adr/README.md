# Architecture Decision Records

ADRは長期的なarchitecture判断の理由とconsequenceを保存します。current contractの詳細は`architecture/`、`domains/`、`interfaces/`に置きます。

## Status

- `Proposed`
- `Accepted`
- `Superseded by ADR-NNNN`
- `Rejected`

## Index

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-use-modular-monoliths.md) | Control PlaneとNode Agentをmodular monolithにする | Accepted |
| [0002](0002-separate-at-machine-boundary.md) | machine boundaryでControl PlaneとNode Agentを分離する | Accepted |
| [0003](0003-use-desired-state-reconciliation.md) | desired-state reconciliationを使用する | Accepted |
| [0004](0004-treat-mutation-timeouts-as-uncertain.md) | mutation timeout中心のuncertainty model | Superseded by ADR-0013 |
| [0005](0005-use-quic-and-json-rpc-for-agent-protocol.md) | raw QUIC、mTLS、JSON-RPC Agent Protocol | Superseded by ADR-0012 |
| [0006](0006-use-private-pki.md) | offline Root CAを持つprivate PKI | Superseded by ADR-0012 |
| [0007](0007-use-akamai-as-initial-compute-provider.md) | initial compute providerをAkamai Cloudにする | Accepted |
| [0008](0008-do-not-build-stateful-provider-fakes.md) | stateful provider fakeを作らない | Accepted |
| [0009](0009-use-podman-quadlet-and-systemd.md) | Server RuntimeにPodman、Quadlet、systemdを使用する | Accepted |
| [0010](0010-use-restic-on-r2-for-server-data.md) | R2上のrestic repositoryをempty passwordで使用する | Superseded by ADR-0014 |
| [0011](0011-use-minecraftserver-as-primary-aggregate.md) | MinecraftServerをprimary aggregate、itzgをsole runtimeにする | Accepted |
| [0012](0012-use-json-rpc-over-http2-agent-pull.md) | JSON-RPC over HTTP/2のAgent pullを使用する | Accepted |
| [0013](0013-use-durable-operations-and-idempotent-agent-commands.md) | durable Operationとidempotent Agent Commandを使用する | Accepted |
| [0014](0014-back-up-server-home-with-restic.md) | Server Homeをresticでbackupし成功contractを信頼する | Accepted |
