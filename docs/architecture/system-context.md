# System context

## Actors and external systems

| Actor/System | Relationship |
| --- | --- |
| Operator | `mcserverctl`またはtrusted local clientを通してdesired stateを変更する |
| Discord Bot | Operator APIを利用するtrusted local process。Discord user authorizationはBot側で行う |
| OCI Compute | Control Planeを常時稼働させるinitial deployment location |
| Akamai Cloud | managed Compute Instanceを提供するinitial compute provider |
| Cloudflare DNS | Agent endpointのstable DNS nameを提供する。proxyせずdirect QUIC endpointへ解決する想定 |
| Cloudflare R2 | restic repositoryを置くinitial object storage backend |
| Node | Node AgentとWorkloadが動くmanaged GNU/Linux machine |
| Minecraft Server | Node Agentのlocal clientから制御・観測されるapplication process |

## Trust boundaries

```text
Operator account / local Unix users
        │
        │ filesystem permission
        ▼
Control Plane trust boundary
        │
        │ private PKI / mTLS
        ▼
Node Agent trust boundary
        │
        ├─ local system interfaces
        ├─ Minecraft local management endpoint
        └─ object storage credentials
```

Discord BotはControl Planeに対してfull-control Operator clientとなり得ます。Discord userごとのpermissionはBot内で検証しますが、Bot process自体が侵害された場合はOperator権限を持つものとして扱います。

## Network posture

- Operator APIはControl Plane VM内のUnix domain socketだけで公開する
- Node AgentはControl Planeへoutbound QUIC connectionを開始する
- managed Nodeへ一般的なinbound management portを公開しない
- Minecraft Server Management ProtocolやRCONは原則としてNode内のloopbackまたはlocal-only networkへ限定する
- break-glass SSHは例外的なoperator recovery手段であり、normal lifecycleやreadinessの依存先にしない
