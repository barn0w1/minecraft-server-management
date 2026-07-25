# Design principles

この文書は、implementation detailより長く維持する判断基準を定義します。

## Minecraft Server is the primary aggregate

Operatorが管理する中心は`MinecraftServer`です。Node、Server Home、runtime、backupはMinecraft Server lifecycleを実現するためのsubsystemです。

Minecraft以外のprogramを実行するgeneric Workload abstractionは作りません。

## Use established components as contracts

restic、Cloudflare R2、systemd、Podman、itzg/minecraft-serverのdocumented behaviorを信頼します。systemはそれらの内部整合性検証を再実装しません。

- restic backup exit code 0を成功とする
- R2のdocumented storage consistencyを前提とする
- systemdをprocess supervision authorityとして利用する
- itzg/minecraft-serverを唯一のMinecraft runtime adapterとする

## Modular monoliths

Control PlaneとNode Agentはmachine boundaryで分離した二つのmodular monolithです。domainごとのmicroservice、message broker、distributed transactionは導入しません。

## Policy above, mechanism below

- Control Planeは何を、いつ、どの順番で行うか決める
- Node AgentはNode上でどう実行し、何が起きたかを報告する

Node Agentはglobal lifecycle policyを所有せず、Control Planeはshell commandやPodman commandを直接構築しません。

## JSON-RPC at process boundaries

Operator APIとAgent APIはJSON-RPC 2.0を共通envelopeにします。独自message framingや独自RPC error envelopeを作りません。

Transportはboundaryごとに選びます。

- Operator API: HTTP/2 over Unix domain socket
- Agent API: HTTPS with HTTP/2

## Agent-initiated communication

Nodeへmanagement portを公開しません。Agentがoutbound syncを開始し、Control Planeはsync resultとしてCommandを返します。

## At-least-once delivery, idempotent effect

response lossを完全になくすことはできません。Control PlaneはCommandを再配送でき、AgentはOperation IDとStageをjournalへ記録して重複effectを防ぎます。

exactly-once deliveryを主張しません。

## Desired state and bounded reconciliation

Controllerはdesired state、Operation、fresh Observationを読み、一回に最大一つの次actionを決めます。eventとin-memory sessionはlatency optimizationであり、restart recoveryの前提ではありません。

## Recover automatically by default

DNS、timeout、429、5xx、Agent disconnect、process restartは通常の運用条件です。bounded backoffで自動retryまたは再観測します。

Incidentはidentity contradiction、multiple active allocation、unknown data overwriteなど、安全な自動選択ができない場合に限定します。

## Server Home is the recovery unit

worldだけをbackup対象にしません。`data/`、runtime manifest、server-local secretを一つのServer Homeとして扱い、Snapshotから同じ起動条件を復元できるようにします。

## Replaceable nodes, fenced ownership

一つのMinecraft Serverに同時に一つのactive Nodeだけを割り当てます。allocationごとのFencing Tokenにより、古いNodeや遅延Commandが新しい状態を変更することを防ぎます。

## Explain current progress

すべてのlong-running mutationはOperationとして表示できなければなりません。Operatorは少なくとも次を確認できます。

- current stage
- last observation
- retry予定
- automatic recoveryの有無
- Operator actionが必要か

## Generalize only from evidence

- generic cloud provider APIを先に作らない
- generic workflow engineを作らない
- future useだけのcrateやtraitを増やさない
- second implementationが現れるまで共通化しない

## Keep destructive rules explicit

Node delete、restore overwrite、Snapshot deleteなどの破壊的操作には明示的preconditionを定義します。通常のreadやretryまで同じ厳格さへ引き上げません。
