# Testing strategy

この文書はimplementation開始後のtest boundaryを定義します。Minecraft automationの重要なfailureを実際のcomponent contractに近い場所で検証します。

## Pure domain tests

対象:

- MinecraftServer Spec validation
- GenerationとCondition derivation
- Operation Stage transition
- retry scheduling
- Fencing Token validation
- Node allocation invariant
- lifecycle orchestration decision

I/O、clock、randomnessはexplicit inputとして扱います。

## SQLite integration and restart tests

production schemaとtransactionを使用し、次を検証します。

- unique active Operation
- unique active Allocation
- optimistic concurrency
- request idempotency
- Operation restart recovery
- Condition/Event persistence
- migration

## Agent API protocol tests

real JSON-RPC codecとHTTP/2 server/clientをloopbackで動かします。

- HTTP/2 negotiation
- authentication and enrollment
- malformed JSON-RPC
- body/deadline/concurrency limit
- long poll timeout
- same Command redelivery
- Agent sequence and Session replacement
- reconnect/backoff

HTTP/1.1-only mockでAgent API behaviorを代替しません。

## Agent journal tests

- Command受理前後のcrash
- effect完了後、result sync前のcrash
- same key/same payload replay
- same key/different payload conflict
- stale Fencing Token rejection
- terminal result acknowledgmentとretention

## Local Node integration

real GNU/Linux test environmentで次を使用します。

- systemd
- Podman
- Quadlet
- itzg/minecraft-server
- RCON
- Server Home filesystem

start、readiness、graceful stop、Agent restart、Control Plane restartをend-to-endで検証します。

## Backup integration

real restic binaryとtemporary local/S3-compatible repositoryを使用します。

- exit code 0とSnapshot ID parse
- exit code 3をfailure扱い
- Server Home全体のbackup/restore
- online save prepare/resumeのfinally behavior
- shared Deployment Restic Password

repository integrity checkerをsystem behaviorとして再実装しません。

## Scripted provider transport tests

production Akamai HTTP encode/decode pathを使用し、responseとtransport failureだけをscriptします。

- pagination
- 429/Retry-After
- complete 4xx
- timeout before response
- create response loss後のinventory
- delete response loss後のGET/NotFound
- malformed response

provider resource state machineをfakeとして再実装しません。

## Real integration and bounded acceptance

- read-only Akamai integrationでauthentication、schema、pagination、type/image/region lookupを確認する
- real mutation testはdedicated Deployment ID、allowlist、hard resource cap、maximum duration、before/after inventory、manual armingを持つ
- cleanupはownership preconditionを必ず通す

## No stateful provider fake

stateful fakeが証明するのはfake自身のsemanticsです。confidenceはdomain test、real HTTP/2 protocol test、restart test、real local runtime、scripted transport、bounded provider acceptanceの組み合わせで得ます。
