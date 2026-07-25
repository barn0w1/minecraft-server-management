# Security model

Security modelはsmall-community deploymentで必要なboundaryを明確にし、private PKIや複雑なcertificate lifecycleをv1の必須要件にしません。

## Security goals

- Operator APIをtrusted local clientへ限定する
- Agentが正しいNode identityとして認証される
- stale NodeのCommand実行をFencing Tokenで拒否する
- managed resource以外を誤って変更しない
- arbitrary remote shellとpublic RCONを提供しない
- credentialをGit、database、normal logへ残さない

## Operator boundary

Operator APIはUnix domain socketで公開します。

```text
/run/mcserver/control-plane.sock
owner: mcserver
group: mcserver-operators
mode: 0660
```

socket accessを持つclientはfull-control Operatorです。Discord userごとのauthorizationはBot側で行います。

## Agent transport

- Agent APIはstable DNS name上のHTTPS endpoint
- HTTP/2をALPN `h2`でnegotiateする
- server identityはpublicまたはdeployment-trusted TLS certificateで検証する
- enrollment後はper-Node Agent Credentialを`Authorization` headerで送る
- Control Planeはcredential digestとactive Node authorizationを検証する
- credentialだけでなくNode ID、Session ID、Fencing Tokenもprotocolで検証する

v1ではclient certificate、offline Root CA、CRL、OCSPを要求しません。必要性が実測された場合にmTLSを追加できます。

## Enrollment

- Control PlaneはNode IDとone-time Enrollment Tokenを発行する
- tokenはrandom、finite TTL、single use
- databaseにはtoken digestだけを保存する
- successful enrollmentでper-Node Agent Credentialを返す
- Agentはcredentialをroot-only fileへ保存する
- Node deleteまたはreplacement時にcredentialをdisableする

詳細は[`interfaces/agent-enrollment.md`](../interfaces/agent-enrollment.md)を参照してください。

## Provider ownership

Akamai resourceにはDeployment ID、Node ID、provision Operation IDを表すmachine-readable metadataを付けます。labelやIP addressだけでownershipを判断しません。

Delete前にはstored Compute Instance IDとownership metadataを再確認します。

## Server Runtime security

- RCONはNode local endpointだけにbindする
- RCON passwordはServer Homeのroot-only `secrets/rcon-password`へ保存する
- itzg runtimeには`RCON_PASSWORD_FILE`として渡す
- Control Planeはarbitrary RCON stringを通常APIとして公開しない
- container image referenceとapplied digestをStatusへ記録する
- Agentはarbitrary shell Commandを受け付けない

## Backup credentials

すべてのrestic repositoryは一つのDeployment Restic Passwordを共有します。

- passwordはOperatorが明示的に設定する
- Control Plane databaseには保存しない
- canonical fileは`/etc/mcserver/secrets/restic-password`
- database lossだけでpasswordを失わない
- disaster recovery時に同じpasswordを再配置できるよう、Operatorがdeployment configurationとして別途保管する
- plaintextをlog、CLI argument、Gitへ出さない

R2 credentialは必要なscopeだけをNode Agentへ提供します。exact delivery mechanismはBackup milestoneで実装contractとして確定します。

## Trust assumptions

- managed Nodeのroot compromiseは、そのNodeへ渡されたserver-local secretとbackup credentialのcompromiseを意味する
- R2 object read権限とDeployment Restic Passwordの両方を得た主体はSnapshotを復号できる
- v1はmalicious Control Planeまたはmalicious root Nodeから防御しない

## Break-glass SSH

break-glass SSHはpublic-key-onlyとし、normal lifecycleやreadinessから独立させます。利用はOperator Eventとして記録します。
