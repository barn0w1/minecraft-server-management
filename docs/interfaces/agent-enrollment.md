# Agent enrollment

## Goal

新しく作成したCompute Instance上のNode Agentへ、logical Node identityにbindingされたprivate keyとshort-lived client certificateを安全に発行します。

## Preconditions

Control PlaneはCompute Instance作成前に次をdurableにします。

- Deployment ID
- Node ID
- expected provider ownership identity
- enrollment token digestとexpiry
- bootstrap revision
- expected Agent endpoint/trust anchor

## Flow

```text
1. Control Plane creates Node identity and one-time token
2. cloud-init receives Node ID, endpoint, Root CA certificate, token, bootstrap revision
3. Node Agent generates its private key locally
4. Agent connects with enrollment ALPN using server-authenticated TLS
5. Agent submits token and CSR
6. Control Plane validates token, Node/provider ownership, CSR proof-of-possession
7. token is consumed atomically
8. Control Plane returns Agent certificate chain
9. Agent reconnects with normal ALPN and mTLS
10. initial report establishes active session
```

Agent private keyはNodeから出しません。

## Token properties

- cryptographically random
- one-time use
- finite TTL
- plaintextをdatabaseへ保存しない
- process argumentやnormal logへ出さない
- successful enrollment後にbootstrap locationから削除する
- replayをControl Plane restart後も拒否する

Tokenは「当該bootstrap instanceの初回enrollment」を証明するだけです。Node内のroot compromiseに対する境界ではありません。

## Certificate profile

- URI SANにcanonical Node identity
- `clientAuth` EKU
- short lifetime
- issuerはAgent issuing intermediate
- certificate identityとdatabase上のactive Node authorizationを両方確認

exact URI format、certificate lifetime、rotation thresholdはNode Management implementation前のP0 decisionです。SPIFFEを採用しない限りSPIFFE namespaceを使用しません。

## Rotation

Agentは新しいprivate keyまたはexisting keyのCSRを生成し、authenticated sessionからrotation requestを送ります。Control PlaneはNode authorization、current certificate、rotation ID、CSR fingerprintを検証します。response loss時はrotation IDから同じresultを再取得できるようにします。

## Recovery

private key喪失、identity rejection、bootstrap contradictionではsilent re-enrollmentを行いません。operator-approved recovery tokenまたはNode replacementのどちらを提供するかはNode Management v1で決定します。

## Deletion

Node release開始時に新規sessionとrotationを禁止し、active sessionをcloseします。Compute Instance Absent確認後にNode identityをfinalizeします。
