# Architecture Decision Records

ADRは長期的なarchitecture判断の理由とconsequenceを保存します。current designの詳細は`architecture/`、`domains/`、`interfaces/`に置きます。

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
| [0004](0004-treat-mutation-timeouts-as-uncertain.md) | external mutation timeoutをuncertainとして扱う | Accepted |
| [0005](0005-use-quic-and-json-rpc-for-agent-protocol.md) | Agent ProtocolにQUIC、mTLS、JSON-RPCを使用する | Accepted |
| [0006](0006-use-private-pki.md) | offline Root CAを持つprivate PKIを使用する | Accepted |
| [0007](0007-use-akamai-as-initial-compute-provider.md) | initial compute providerをAkamai Cloudにする | Accepted |
| [0008](0008-do-not-build-stateful-provider-fakes.md) | stateful provider fakeを作らない | Accepted |
| [0009](0009-use-podman-quadlet-and-systemd.md) | Workload RuntimeにPodman、Quadlet、systemdを使用する | Accepted |
| [0010](0010-use-restic-on-r2-for-server-data.md) | Server Data backupにrestic repository on R2を使用する | Accepted |
