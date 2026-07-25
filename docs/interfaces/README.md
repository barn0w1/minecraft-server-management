# Interfaces

このdirectoryはprocess boundaryとexternal protocolのcurrent contractを定義します。

- [Operator API](operator-api.md): `mcserverctl`、Discord Bot、local automationからControl Plane
- [Agent Protocol](agent-protocol.md): Control PlaneとNode Agent
- [Agent Enrollment](agent-enrollment.md): Node Agentの初回identity発行
- [Minecraft Server Control](minecraft-server-control.md): Node Agentとlocal Minecraft process

JSON-RPCはmessage envelopeであり、transport、authentication、authorization、schema、retry、idempotency、durabilityを単独では定義しません。それぞれのinterface documentで周囲のcontractを明示します。
