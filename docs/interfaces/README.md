# Interfaces

このdirectoryはprocess boundaryのcurrent contractを定義します。

- [Operator API](operator-api.md): Operator ClientからControl Plane
- [Agent API](agent-protocol.md): Node AgentからControl Planeへのsync
- [Agent Enrollment](agent-enrollment.md): Node Agentの初回credential発行
- [Minecraft Server Control](minecraft-server-control.md): Node Agentとlocal itzg runtime/RCON

Operator APIとAgent APIはJSON-RPC 2.0を共通envelopeとして使用します。

JSON-RPCが定義するのはrequest、response、error、notificationの形です。HTTP transport、authentication、Operation ID、retry、idempotency、schema versionは各interface documentで定義します。
