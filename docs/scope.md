# Scope

## System scope

このsystemは次を管理します。

- Akamai Cloud上のCompute Instance provisioningと削除
- GNU/Linux Nodeのbootstrap、identity、observation、readiness
- Node Agentのenrollment、mTLS session、local operation
- Podman、Quadlet、systemdを利用したWorkload lifecycle
- Minecraft Serverのconfiguration、readiness、save、stop、状態観測
- Server Dataのbackup、restore、snapshot、retention、verification
- 上記を協調させるMinecraft Server lifecycle orchestration
- Unix domain socket上のOperator API
- `mcserverctl`、Discord bot、local automationなどのtrusted local client

## Initial deployment profile

- Control PlaneはOCI Compute上で常時稼働する一つのprocess
- managed NodeはAkamai CloudのCompute Instance
- Node Agentはmanaged Node上に常駐し、Control Planeへoutbound接続する
- Agent endpointはstable DNS nameを使用し、Control PlaneがQUIC/TLSを直接終端する
- persistent backup backendはCloudflare R2
- deploymentはsmall-community向けのsingle-operator-domainを基本とする

これらは最初のdeployment profileであり、generic multi-cloud platformの約束ではありません。

## Explicit non-goals

現在は次を目標にしません。

- public hosting SaaS
- multi-tenant isolation、billing、self-service signup
- enterprise policy engine
- general-purpose container orchestrator
- arbitrary remote shell platform
- generic workflow engine
- generic cloud provider plugin ecosystem
- Control Planeのhigh availability
- public remote Operator API
- web dashboardを初期要件にすること
- Node pool、bin packing、automatic workload migration
- old prototypeとのbackward compatibility

## Development compatibility

最初のstable releaseまでは、RPC、database、configuration、CLI、certificate profile、resource modelの後方互換性を保証しません。変更が必要な場合、compatibility shimやdual-writeよりcurrent designの単純さと正しさを優先します。

## Milestone discipline

全systemを同時に実装しません。最初はfoundationとNode Managementを構築し、そのcontractの上へWorkload、Server Data、Minecraft Serverを追加します。現在のplanは[`plans/`](plans/README.md)を参照してください。
