# Historical design lineage

この文書はoptionalなhistorical contextです。Minecraft Server Management Systemのcurrent designを理解、実装、reviewするために読む必要はありません。normativeな判断はcurrent design documentとADRだけに置きます。

## Earlier experiments

このrepository以前に、Python prototypeと`mc-control-plane` Rust prototypeでCloud resource lifecycle、Agent enrollment、PKI、transport、failure recoveryを検討しました。

それらのcode、schema、module layout、名称、文書はこのrepositoryのsource of truthではありません。新しい実装へ互換性目的で持ち込みません。

## Knowledge retained

過去の実験から、現在の設計へ次の知見を再評価して取り入れています。

- logical identityとprovider identityを分離する
- desired state、durable intent、Observation、Statusを分離する
- Controllerはlevel-triggered reconciliationを行う
- mutation response lossをfailureと断定せずuncertainとして扱う
- ownership contradictionやduplicate identityではmutationを止める
- provider Absent確認前にresourceをfinalizeしない
- process restart後にdurable stateから再収束する
- Agent identityをprivate PKIとserver-side authorizationでbindingする
- heartbeatとfull reportを分離する
- Node Agentはjitter付きexponential backoffでreconnectする
- stateful provider fakeを作らず、境界ごとのtestとbounded real acceptanceを組み合わせる
- Nodeは交換可能、Server Dataは永続とする
- small-community向けでもdata safetyとdestructive mutation safetyを妥協しない

## Intentionally reset

新repositoryでは次を継承しません。

- Python implementationとそのmodule layout
- 旧Rust prototypeのcrate、schema、RPC、fake provider
- `Host`、`HostClaim`、`control`などの旧名称
- checkpointごとに増えた重複requirements
- superseded ADRをcurrent contractとして扱うこと
- future featureを先取りしたgeneric abstraction
- stable release前のbackward compatibility

## Rule for future work

過去のartifactを参照する必要が生じても、文章やimplementationをcopyしてcurrent designへ戻しません。failure caseやinvariantを再評価し、現在のterminology、domain boundary、security modelに適合する形で新たに記述します。
