# Process model

## `mcserver-control-plane`

配置: initial deploymentではOCI Compute

所有するもの:

- SQLite application database
- MinecraftServer Spec、Generation、Status
- Node identity、allocation、provider binding
- durable Operation、Condition、Event、Incident
- reconciliation scheduling
- Akamai API credential access
- Operator API Unix socket
- Agent API HTTPS endpoint
- Node enrollment tokenとAgent credential hash
- Deployment Restic Passwordへのaccess

Control Plane restart後はdatabaseからactive Operationとretry scheduleを再構築します。memory上のAgent Sessionは失われたものとして扱い、新しいAgent Syncでlivenessを回復します。

## `mcserver-node-agent`

配置: managed Node

所有するもの:

- Node IDとAgent credential
- Agent Session ID
- HTTP/2 connectionとsync loop
- local operation journal
- Server Home filesystem
- materialized Quadlet files
- local runtime、RCON、restic operation
- Node、runtime、MinecraftのObservation

AgentはControl Planeとのconnection loss後にjitter付きbackoffで再接続します。Control Planeからcommandをpushされるためのinbound portは持ちません。

## Agent concurrency

Agentは少なくとも次を分離します。

- sync loop
- one active mutating Operation Stage per MinecraftServer
- local observation loop
- runtime process supervisionはsystemdへ委譲

HTTP/2によりsyncとresult reportingは同じconnection上のseparate streamとして並行できます。

## `mcserverctl`

配置: Control Planeと同じhostまたはOperator socketへaccessできるtrusted environment

責務:

- Operator API typed client
- argument validation
- human-readable status、Operation、Event表示
- request correlation

SQLite、Akamai、Agentへ直接接続しません。

## Discord Bot and local automation

`mcserverctl`と同じOperator APIを使用します。Discord user authorizationはBot側で行います。Control PlaneはUnix peer identityをauthentication actorとして記録し、Botが申告したDiscord actor metadataをaudit contextとして追加できます。

## External processes on a Node

Node Agentは次を利用します。

- systemd
- Podman
- itzg/minecraft-server container
- local RCON client
- restic subprocess

Node Agentはtyped adapterを所有し、external command lineをAgent APIへ露出させません。
