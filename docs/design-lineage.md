# Design lineage

このrepositoryはclean-room implementationを目指しますが、過去の試行錯誤を無かったことにはしません。

## Sources of experience

- 初期Python prototype
- 旧`mc-control-plane` Rust prototype
- Akamai Cloud resource lifecycleの設計とfailure analysis
- Host/Agent enrollment、PKI、QUIC protocolの設計review
- Minecraft、Workload、Server Dataを一つの巨大なcontrollerへ混ぜないためのdomain再整理

これらはreferenceであり、現在のsource of truthではありません。

## Knowledge carried forward

- logical identityとprovider identityを分離する
- desired state、durable intent、Observation、Statusを分離する
- Controllerはlevel-triggered reconciliationを行う
- mutation response lossをfailureと断定せずuncertainとして扱う
- ownership contradictionやduplicate identityではmutationを止める
- provider Absent確認前にresourceをfinalizeしない
- process restart後にdurable stateから再収束する
- Agent identityをprivate PKIとserver-side authorizationでbindingする
- heartbeatとfull reportを分離する
- Agentはjitter付きexponential backoffでreconnectする
- stateful provider fakeを作らず、境界ごとのtestとbounded real acceptanceを組み合わせる
- Nodeは交換可能、Server Dataは永続とする
- small-community向けでもdata safetyとdestructive mutation safetyを妥協しない

## Intentionally reset

新repositoryでは次を引き継ぎません。

- Python implementationとそのmodule layout
- 旧Rust prototypeのcrate、schema、RPC、fake provider
- `Host`、`HostClaim`、`control`などの旧命名を互換性目的で維持すること
- checkpointごとに増えた重複requirementsと同じ事実の多重記載
- superseded ADRをcurrent contractとして扱うこと
- future featureを先取りしたgeneric abstraction
- stable release前のbackward compatibility

## Rule

過去のdocumentから文章をcopyするのではなく、現在も成立するinvariantまたはdecisionを新しいterminologyとarchitectureで再記述します。過去repositoryは議論と実験のhistoryとして保存し、新repositoryは現在の設計だけを簡潔に説明します。
