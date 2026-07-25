# Node domain

## Purpose

Minecraft Serverを実行できるidentity付きGNU/Linux machineを登録またはAkamai Cloud上に作成し、一つのMinecraftServerへallocateして安全に解放します。

## Owned concepts

- `Node`
- `NodeSpec`
- `NodeStatus`
- `ComputeInstanceBinding`
- `Allocation`
- `FencingToken`
- `AgentCredentialAuthorization`

## v1 invariants

- 一つのMinecraftServerにactive Allocationは最大一つ
- 一つのNodeにactive Allocationは最大一つ
- Allocationごとにmonotonic Fencing Tokenを発行する
- Node Agent credentialは一つのNode IDへbindingする
- Node IDとCompute Instance IDを混同しない
- ownedと確認できないCompute Instanceをmutationしない

## Node sources

v1は二種類のNode sourceを扱います。

### Registered Node

Operatorが事前に用意し、Node Agentをinstallして登録するNodeです。最初のvertical sliceで使用します。

### Akamai Node

Control PlaneがAkamai APIでCompute Instanceをcreateし、bootstrapとAgent enrollmentを行うNodeです。後続milestoneで追加します。

両者は上位MinecraftServerから同じ`NodeAvailable` contractとして利用しますが、provision/delete capabilityは異なります。

## Readiness

`NodeAvailable=True`には次を要求します。

- Node authorizationがactive
- Agent Syncがfresh
- required capability reportを受理済み
- current Allocationまたはunallocated stateが矛盾していない
- blocking Incidentがない

Akamai Nodeではさらにowned Compute Instanceがexpected provider bindingと一致することを要求します。

## Allocation

MinecraftServerをNodeへ割り当てるtransactionで次を保存します。

```text
minecraft_server_id
node_id
fencing_token
allocated_at
released_at
```

Agent CommandはNode ID、MinecraftServer ID、Fencing Tokenを含みます。Agentはlocal accepted tokenより古いCommandを拒否します。

## Akamai lifecycle

```text
Requested
  → Provisioning
  → Bootstrapping
  → Enrolling
  → Available
  → Releasing
  → Deleting
  → Absent
```

provider create responseを失った場合はDeployment ID、Node ID、Operation IDに対応するmetadataでinventoryします。duplicateそのものを即Incidentにせず、ownedかつ安全にcleanupできる余剰resourceはOperationとして処理します。一意にcurrent resourceを決められない場合だけIncidentです。

## Delete preconditions

Akamai Compute Instance deleteには次を要求します。

- stored Compute Instance IDが存在する
- ownership metadataがDeployment IDとNode IDに一致する
- active Allocationがない
- MinecraftServer policyが要求するbackup Operationが成功済み
- Node authorizationをdisable済み

Delete responseを失った場合はsame IDをreadし、NotFoundならsuccessとします。

## Non-responsibilities

- Minecraft configuration
- RCON command semantics
- Server Home backup content
- generic Node pool、bin packing、multi-server scheduling
- arbitrary shell execution
