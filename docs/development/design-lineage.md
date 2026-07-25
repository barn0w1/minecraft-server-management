# Historical design lineage

この文書はoptionalなhistorical contextです。current designの理解、implementation、reviewに必須ではありません。normative contractはcurrent design documentとADRだけに置きます。

## Earlier direction

初期documentationでは、Node、Workload、Server Data、Minecraft Serverを対等なdomainとして扱い、raw QUIC、private PKI、細かなuncertainty classification、backup verificationをfoundationから実装する計画でした。

それらはdistributed failureを真剣に扱う長所がありましたが、Minecraft Serverを実際に起動・停止するまでのscopeを大きくし、通常failureでもautomationが止まりやすい構造でした。

## Architecture Reset

Milestone 0で次を変更しました。

- MinecraftServerをprimary aggregateにした
- itzg/minecraft-serverを唯一のruntimeとした
- generic Workload domainを削除した
- `/data`とruntime configurationをServer Homeへ統合した
- Agent APIをJSON-RPC over HTTP/2のpull modelへ変更した
- private PKIをper-Node credentialへ簡素化した
- uncertainty中心のmodelをdurable Operationとidempotent replayへ変更した
- Incidentをunsafe contradictionへ限定した
- restic successful completionをbackup contractとして信頼した
- one Deployment Restic Passwordを全repositoryで共有した
- implementationをLocal Node vertical sliceから始める計画へ変更した

## Knowledge retained

- logical identityとprovider identityを分離する
- desired stateとObservationを分離する
- Controllerはlevel-triggered reconciliationを行う
- process restart後にdurable stateから再開する
- Nodeは交換可能にする
- destructive operationにはownershipとallocation preconditionを要求する
- stateful provider fakeを作らない
- arbitrary remote shellを提供しない

## Rule for future work

過去のartifactをcompatibility目的でcopyしません。current terminology、current resource model、current protocolに適合する必要性がある場合だけ再設計します。
