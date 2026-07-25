# Design principles

この文書は、実装詳細より長く維持される判断基準を定義します。

## Modular monoliths

Control PlaneとNode Agentは、それぞれ一つのprocessとして動くmodular monolithです。二つのprocessが分かれる理由はmachine boundaryであり、domainごとのmicroservice化ではありません。

```text
Control Plane modular monolith  <— network boundary —>  Node Agent modular monolith
```

内部moduleは責務、state ownership、dependency directionを明確にします。独立deployment、scale、failure isolationが実証されるまでnetwork serviceへ分割しません。

## Policy above, mechanism below

- Control Planeは「何を、なぜ、どの順番で行うか」を決める
- Node Agentは「Node上でどう安全に実行し、何が観測されたか」を扱う

Node AgentはMinecraft、restic、Podmanを知ってよいですが、serverをいつ停止するか、backup成功前にNodeを解放してよいかといった全体policyは所有しません。

## Explicit domain ownership

主要domainは次です。

```text
Minecraft Server
Server Data
Workload
Node
```

下位moduleは上位domainを知りません。

- NodeはMinecraftを知らない
- WorkloadはMinecraftのsave semanticsを知らない
- Server DataはMinecraft fileの内容を解釈しない
- Minecraft Serverはprovider APIやrestic commandを直接呼ばない

## Desired state and observed state

command responseやAPI responseだけで実世界の状態を確定しません。desired state、persisted intent、external observation、derived statusを区別します。

## Level-triggered reconciliation

eventはlatencyを下げるhintです。event deliveryを正しさの前提にせず、restart後もdurable stateとfresh observationから再評価できるlevel-triggered controllerを使用します。

## External mutation can be uncertain

network timeoutやconnection lossの後、mutationが実行されなかったとは限りません。blind retryせず、read-only observationから結果を確定します。確定不能またはidentity contradictionならaffected scopeのmutationを停止します。

## Safety over automatic progress

安全に判断できない場合は停止し、Incidentとして理由を残します。不可視な無期限retry、ownershipを無視したcleanup、推測によるsuccess transitionを行いません。

## Durable data, replaceable nodes

Node lifecycleとServer Data lifecycleを分離します。Nodeを失っても、verified Snapshotから新しいNodeへrestoreできる設計を目指します。

## Narrow external ownership

- Control Plane databaseへのwrite ownerはControl Planeだけ
- Akamai mutation ownerはNode subsystemだけ
- Node上のlocal mutation ownerはNode Agentだけ
- Operator clientとDiscord botはdatabaseやproviderへ直接接続しない

## Generalize from evidence

- generic provider abstractionを先に作らない
- stateful cloud simulatorを作らない
- future useだけのtraitを増やさない
- generic workflow engineを作らない
- actual second implementationまたは明確なtest seamが生じた時点で抽象化する

## Bounded resources

retry、stream、payload、buffer、concurrency、operation durationには有限上限を持たせます。default値とprotocol invariantを区別し、実測で調整可能にします。
