# System context

## Actors and external systems

| Actor/System | Relationship |
| --- | --- |
| Operator | `mcserverctl`またはtrusted local clientを通してdesired stateを変更する |
| Discord Bot | Operator APIを利用するtrusted local process。Discord user authorizationはBot側で行う |
| OCI Compute | Control Planeを常時稼働させるinitial deployment location |
| Akamai Cloud | managed Compute Instanceを提供するinitial compute provider |
| Cloudflare DNS | Agent API endpointのstable DNS nameを提供する |
| Cloudflare R2 | restic repositoryを保持するobject storage backend |
| Node | Node AgentとServer Runtimeが動くmanaged GNU/Linux machine |
| itzg/minecraft-server | Minecraft Server processを構成・起動する唯一のruntime image |

## Trust boundaries

```text
Operator / local Unix account
        │
        │ filesystem permission
        ▼
Control Plane trust boundary
        │
        │ HTTPS / HTTP/2
        │ per-Node credential
        ▼
Node trust boundary
        │
        ├─ systemd / Podman
        ├─ local-only RCON
        └─ scoped backup credential
```

Control Planeとmanaged Nodeは同じDeploymentに属するtrusted componentsですが、network failureとstale Nodeは発生するものとして扱います。

## Network posture

- Operator APIはUnix domain socketだけで公開する
- Node AgentがControl Planeへoutbound HTTPS connectionを開始する
- Agent APIはALPN `h2`でHTTP/2を使用する
- managed Nodeへ一般的なinbound management APIを公開しない
- RCONはNode local endpointだけにbindし、public portへpublishしない
- Minecraft game portだけをplayer accessのために公開する
- break-glass SSHはnormal automationから独立したoperator recovery手段とする

## External trust assumptions

- Akamai API responseとinventoryをprovider stateのauthorityとする
- successful restic commandをbackup resultのauthorityとする
- Cloudflare R2のdocumented consistencyとdurabilityを信頼する
- systemdをlocal process supervisionのauthorityとする
- itzg/minecraft-serverのdocumented `/data`、RCON、health behaviorを利用する
