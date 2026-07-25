# Agent enrollment

## Goal

新しいNode Agentへlogical Node identityにbindingされたAgent Credentialを一度だけ発行し、通常のAgent APIを利用可能にします。

## Preconditions

Control Planeは次をdurableにします。

- Deployment ID
- Node ID
- expected Node sourceまたはprovider binding
- Enrollment Token digest
- token expiry
- token state

## Flow

```text
1. Control Plane creates Node ID and Enrollment Token
2. Node receives endpoint, Node ID, token, server trust configuration
3. Agent validates Control Plane TLS certificate
4. Agent calls agent.enroll over HTTPS/HTTP/2
5. Control Plane validates token digest, expiry, Node state
6. token is consumed atomically
7. Control Plane returns per-Node Agent Credential
8. Agent stores credential in root-only file
9. Agent starts authenticated agent.sync loop
10. first accepted sync makes AgentAvailable observable
```

## Enrollment Token

- cryptographically random
- one-time use
- finite TTL
- plaintextをdatabaseへ保存しない
- process argumentとnormal logへ出さない
- successful enrollment後にbootstrap locationから削除する
- Control Plane restart後もreplayを拒否する

## Agent Credential

- cryptographically random bearer credential
- one Node IDへbindingする
- Control Planeはdigestだけをdatabaseへ保存する
- Agentは`/etc/mcserver/secrets/agent-credential`などのroot-only fileへ保存する
- rotationはOperatorまたはfuture maintenance Operationで行える
- Node release/replacement時にauthorizationをdisableする

v1ではclient certificateとprivate keyを必要としません。

## Recovery

credential loss時はsilent re-enrollmentしません。Operatorがold authorizationをdisableし、新しいEnrollment Tokenを発行します。

Akamai on-demand Nodeでは、credential recoveryよりNode replacementをdefaultにできます。

## Deletion

Node delete開始時にAgent authorizationをdisableし、新しいCommand allocationを停止します。Agentが接続中でもold Fencing TokenのCommandを拒否できるよう、Allocationをreleaseします。
