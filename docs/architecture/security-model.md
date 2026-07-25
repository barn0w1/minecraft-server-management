# Security model

## Security goals

- Control PlaneとNode Agentが相互に正しいdeployment/node identityを検証する
- managed resource以外を誤って変更しない
- Operator APIをlocal trusted clientへ限定する
- credentialとprivate keyをdatabase、log、Gitへ漏らさない
- Node削除後に古いAgent credentialで操作できない
- break-glass accessをnormal lifecycleから分離する

## Operator boundary

Operator APIはUnix domain socketで公開し、filesystem permissionとOS peer identityをaccess boundaryとします。初期構成ではTLSを使用しません。

```text
/run/mcserver/control-plane.sock
owner: mcserver
group: mcserver-operators
mode: 0660
```

Discord Botはこのsocketへaccessできるtrusted processです。Discord側permissionはBotが実施します。

## Agent transport identity

- Control Plane server certificateはAgent endpointのDNS SANと`serverAuth`を持つ
- enrollment後のNode Agent certificateはNode identityを表すURI SANと`clientAuth`を持つ
- certificate chainだけでなく、database上のactive Node authorizationと一致することを要求する
- exact URI encodingはimplementation開始前にinterface contractでcanonicalに固定する
- SPIFFEを正式採用しない限り、SPIFFE IDを装わない

## Private PKI

推奨hierarchy:

```text
Offline Root CA
  ├─ Server Issuing Intermediate
  │    └─ Control Plane server leaf
  └─ Node Agent Issuing Intermediate
       └─ short-lived Node Agent leaf
```

Root CA private keyはrunning Control Plane、managed Node、database、Git、R2、通常backupへ置きません。offline operator environmentまたは暗号化removable mediaで保管します。

Control PlaneはNode Agent certificateを自動更新するためのonline issuing materialを持ち得ます。server certificate issuerとAgent issuerのkey/profileを分離します。

## Enrollment

- Node identityはCompute Instance作成前にControl Planeが発行する
- bootstrapへDeployment trust anchor、Node ID、endpoint、one-time tokenを渡す
- Node Agentは自身でprivate keyを生成し、CSRを送る
- one-time tokenはdigestだけをdatabaseへ保存し、atomicに一回だけconsumeする
- enrollment後は新しいmTLS connectionへ切り替える
- silent automatic re-enrollmentは行わない

詳細は[`interfaces/agent-enrollment.md`](../interfaces/agent-enrollment.md)を参照してください。

## Provider ownership

Akamai resourceにはDeployment IDとNode IDへ対応するmachine-readable ownership tagを付けます。labelやIP addressだけで所有権を判断しません。mutation前とfinalization前にownershipを再検証します。

## Secrets

- Akamai credential、R2 credential、issuing keyはdaemon-readable secret fileまたは専用secret storeから読み込む
- command-line argument、environment dump、structured log、databaseへplaintext secretを残さない
- Node Agentへ必要なcredentialだけをscope限定して渡す
- evidenceとaudit recordはredaction済みとする

## Break-glass SSH

break-glass SSHはpublic-key-only、operator CIDR制限、normal automation非依存とします。利用はIncident responseとしてauditし、通常のNode readiness条件に含めません。

## Revocation model

初期実装ではCRL/OCSPへ依存せず、short-lived Agent certificate、server-side active Node authorization、rotation停止、active connection切断を組み合わせます。Node deletion開始時にauthorizationを無効化し、certificateが期限内でも新規sessionを拒否します。
