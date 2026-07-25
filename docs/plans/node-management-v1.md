# Node Management v1 plan

Status: Proposed

## Goal

exact Akamai compute typeの要求から、一台のowned Compute Instanceを作成し、GNU/Linuxをbootstrapし、Node Agentをenrollさせ、fresh observationを持つ`Ready` Nodeとして提供し、最後に安全にdelete/Absent確認できるようにします。

```text
Node desired Present
  → owned Compute Instance
  → reproducible bootstrap
  → Node Agent enrollment
  → mTLS session and fresh observation
  → Ready Node
  → desired Absent
  → delete and provider Absent confirmation
```

## In scope

- Node logical identity
- exact Akamai type ID
- deployment-owned region/image/network/bootstrap policy
- Akamai typed HTTP client
- ownership tags and inventory
- create/delete uncertainty handling
- Debian 13 GNU/Linux bootstrap contract
- one-time Agent enrollment
- Agent heartbeat/full report/reconnect
- Node readiness
- normal release/delete/finalization
- Incident and restart recovery

## Out of scope

- Workload Runtime
- Podman/Quadlet
- restic/R2
- Minecraft Server control
- Node reuse/pool/scheduling
- rebuild/resize/repair
- arbitrary shell RPC
- multi-provider support

## P0 decisions before implementation

- Node resourceとseparate request/claim resourceの必要性
- exact Node certificate URI SAN format
- initial certificate lifetime/rotation threshold
- bootstrap artifact distribution and integrity verification
- cloud-init completion record schema
- enrollment token format/storage/transport
- Node Agent private key recovery policy
- initial QUIC limits/deadlines/default timing
- exact ownership tag format under provider limits

## Implementation slices

1. Node domain model、SQLite schema、pure transition tests
2. failure classification、Incident、mutation intent
3. Akamai HTTP clientとscripted transport tests
4. read-only real Akamai integration
5. create/inventory/reconcile with ownership proof
6. bootstrap material and completion observation
7. private PKI enrollment and active Agent session
8. heartbeat/report/readiness/reconnect
9. normal release/delete/Absent finalization
10. bounded real lifecycle acceptance

## Acceptance criteria

- process restart中にduplicate Compute Instanceをblind createしない
- create response loss後にinventoryからexactly one owned instanceをrecoverできる
- duplicate/ownership contradictionではIncidentを作りmutationを停止する
- certificate identityとNode identityが一致しないsessionを拒否する
- Control Plane restart後、新sessionとinitial report前にReadyへ戻さない
- heartbeat staleでReadyを失う
- Agentはnetwork interruption後にjitter付きbackoffで再接続する
- Node deletion時にauthorizationを停止する
- provider Absent確認前にfinalizeしない
- unrelated Nodeとread-only observationはblocking Incident中も継続する

## Exit condition

上位domainがAkamai、bootstrap、certificate、heartbeatのdetailを知らずに、NodeのPresent/Ready/Absent contractだけを利用できること。
