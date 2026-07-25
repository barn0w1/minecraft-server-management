# Process model

## `mcserver-control-plane`

配置: initial deploymentではOCI Compute

所有するもの:

- application database
- desired stateとstatus
- controller scheduling
- durable operationとIncident
- Akamai API credential access
- Agent issuing authorityとserver TLS material
- Operator API Unix socket
- Agent QUIC endpoint

process restart後はdatabaseとexternal observationから再収束します。memory上のAgent sessionやheartbeatは失われたものとして扱います。

## `mcserver-node-agent`

配置: managed Node

所有するもの:

- Node identity private key
- Control Plane connection lifecycle
- local operation execution
- local capability discovery
- local operation journalに必要な最小state
- Node、Workload、Server Data、Minecraftのobservation

Node AgentはControl Planeとのconnection loss後、自律的にreconnectします。ただしidentity rejectionやexplicit disablementを一時的network failureとして高速retryしません。

## `mcserverctl`

配置: Control Planeと同じVMまたはUnix socketへaccessできるtrusted local environment

責務:

- Operator APIのtyped client
- human-readable output
- command argument validation
- request correlation

database、Akamai API、Node Agentへ直接接続しません。

## Discord Bot and local automation

最初は`mcserverctl`と同じOperator API、同じUnix socket、同じControl Plane権限を使用します。Bot固有のDiscord role/user authorizationはBot側で行います。Control Planeのaudit recordには、Unix peer identityに加えてBotが申告したactor metadataを保存できますが、両者を同一のauthentication proofとはみなしません。

## Child and external processes on a Node

Node Agentは必要に応じて次を利用します。

- systemd
- Podman
- restic subprocess
- Minecraft Server Management Protocol client
- RCON client

外部command lineの文字列をControl Plane RPCへ露出させず、Node Agent内のtyped adapterで変換します。
