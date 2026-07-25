# ADR-0012: Use JSON-RPC over HTTP/2 with Agent-initiated pull

Status: Accepted

Supersedes: ADR-0005, ADR-0006

## Context

Control PlaneとNode Agentの間にはtyped RPC、connection reuse、concurrency、outbound-only Node networking、reconnect recoveryが必要です。

raw QUICとprivate PKIはこれらを実現できますが、custom framing、stream mapping、certificate lifecycleの実装範囲が大きくなります。一方、JSON-RPCはproject内のRPC envelopeとして一貫して利用できます。

## Decision

- Agent APIはJSON-RPC 2.0を使用する
- transportはHTTPS上のHTTP/2とする
- Agentがすべてのrequestを開始する
- Control Planeは`agent.sync` resultとしてtyped Commandを返す
- idle時はHTTP long pollingを使用する
- server identityはTLS certificateで検証する
- Agent identityはone-time enrollment後のper-Node bearer credentialで検証する
- private PKIとclient certificateはv1要件にしない

## Consequences

standard HTTP server/client、TLS、proxy、logging、packet inspectionを利用できます。Control PlaneからAgentへ直接RPCを開始できないため、Command deliveryはpoll latencyを持ちますが、Minecraft lifecycleには十分です。

JSON-RPCだけではidempotencyを保証しないため、Operation IDとAgent journalを別contractとして定義します。

## Related documents

- [Agent API](../interfaces/agent-protocol.md)
- [Agent enrollment](../interfaces/agent-enrollment.md)
