# Testing strategy

この文書は実装開始後のtest boundaryを先に定義します。statefulな外部system simulatorを作るのではなく、各責務に合ったtestを組み合わせます。

## Pure domain tests

対象:

- state transition
- readiness derivation
- failure classification
- mutation gate
- lifecycle orchestration decision
- immutable spec/invariant

I/O、clock、randomnessはexplicit inputとして扱います。

## SQLite integration and restart tests

production schemaとtransactionを使用し、次を検証します。

- constraint
- optimistic concurrency
- operation intent durability
- Incident lifecycle
- process restart後のresumption
- migration

## Scripted transport tests

production HTTP/QUIC/JSON encode/decode pathを使用し、transport responseやfailureだけをscriptします。

Akamai test例:

- pagination
- 429/Retry-After
- complete 4xx
- timeout before/after send
- malformed/truncated 2xx
- unknown provider status

provider resource state machineを再実装しません。

## Agent protocol harness

real TLS profile、framing、JSON-RPC dispatcherをin-processまたはloopbackで動かし、次を検証します。

- enrollment
- mTLS identity
- stream mapping
- malformed frame
- session replacement
- heartbeat freshness
- reconnect/backoff logic
- request timeoutとuncertain operation

## Real integration

read-only Akamai integrationでauthentication、schema、pagination、type/image/region lookupを確認します。

## Bounded lifecycle acceptance

real resource mutationは、dedicated Deployment ID、allowlist、hard resource cap、maximum duration、before/after inventory、manual armingを持つtestだけで行います。failure時もownershipを無視したcleanupを自動実行しません。

## No stateful provider fake

stateful fakeはproduction providerのsemanticsを不完全に再実装し、test自体のmaintenance負債を増やします。必要なconfidenceはdomain test、transport fixture、restart test、real acceptanceの組み合わせで得ます。
