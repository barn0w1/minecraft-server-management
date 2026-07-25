# Workload domain

## Purpose

Node上でMinecraft Serverなどのprogramを安全かつ再現可能に実行する基盤を提供します。

## Owned concepts

- `Workload`
- `WorkloadSpec`
- `WorkloadRevision`
- `WorkloadStatus`
- `WorkloadOperation`
- `RuntimeObservation`

## Responsibilities

- container imageまたはexecutableの指定
- environment、mount、port、user、resource limit
- desired revisionの適用
- Podman/Quadlet/systemd unit lifecycle
- start、stop、restart、remove
- process/container/systemd stateの観測
- runtime readinessとfailureの正規化
- previous revisionからの安全なtransition

## Node Agent side

Node Agentの`workload` moduleはWorkload Runtimeとして動きます。

```text
Workload operation
  → validated local specification
  → Quadlet materialization
  → systemd daemon reload
  → unit start/stop
  → Podman/systemd observation
```

implementation detailをControl Plane RPCへ漏らさず、typed operationとobservationへ変換します。

## Non-responsibilities

- Minecraft save、player、allowlist、game rule
- backup repository、Snapshot、retention
- Akamai Compute Instance provisioning
- Server Dataの内容解釈
- arbitrary operator-provided shell command

Minecraft ServerはWorkloadの一種として実行されますが、Workload domainはMinecraft固有の意味を知りません。
