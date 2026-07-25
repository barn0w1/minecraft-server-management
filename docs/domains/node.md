# Node domain

## Purpose

Akamai CloudのCompute Instanceを、上位domainが利用できるidentity付き・観測可能・agent-readyなGNU/Linux Nodeへ変換し、安全に解放します。

## Owned concepts

- `Node`
- `NodeSpec`
- `NodeStatus`
- `NodeObservation`
- `NodeIncident`
- `AgentEnrollment`
- `AgentSession`
- `ComputeInstanceBinding`

初期実装で`NodeClaim`を別resourceとして導入するかは、Node Management v1 planで必要性を判断します。過去のresource名を互換性目的で維持しません。

## Responsibilities

- logical Node identityの発行
- exact Akamai compute type、region、imageなどのprovisioning input管理
- Compute Instanceのcreate、inventory、delete
- ownership tagとexternal identityの検証
- GNU/Linux bootstrap contract
- Node Agent enrollmentとauthorization
- provider、bootstrap、Agentのobservation
- Node readinessの導出
- release、delete、provider Absent確認、finalization

## Readiness

`Ready`は単一のprovider statusではありません。少なくとも次を満たす必要があります。

- owned Compute Instanceが存在し、expected identity/type/regionと一致する
- bootstrap completionが成功として観測される
- active Node Agent sessionがmTLSで認証される
- heartbeatがfreshである
- required initial reportが受理されている
- Node-level blocking Incidentが存在しない

## Non-responsibilities

- Minecraft playerやgame rule
- Minecraft save/stop semantics
- restic retention policy
- Workload container specificationの意味
- Nodeを複数Workloadへpackingするscheduler
- arbitrary shell execution API

## Provider boundary

initial providerはAkamai Cloudです。generic provider plugin systemを先に作らず、Node application layerからprivateなAkamai adapterを呼びます。

```text
NodeController
  → Compute observation/mutation port
  → AkamaiComputeAdapter
  → Linode API client
```

`Node ID`と`Compute Instance ID`は別identityです。

## Lifecycle sketch

```text
Requested
  → Provisioning
  → Bootstrapping
  → Enrolling
  → Ready
  → Releasing
  → Deleting
  → Absent
```

phase名は実装前にstate modelとともに確定し、phaseだけで細かなconditionを隠しません。
