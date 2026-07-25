# Minecraft Server domain

## Purpose

Minecraft固有のdesired lifecycle、itzg configuration、application readiness、save、stop、player observation、automation policyを管理します。

## Owned concepts

- `MinecraftServer`
- `MinecraftServerSpec`
- `MinecraftServerStatus`
- `Generation`
- `Server Runtime`
- Minecraft-specific Condition and Event

`Operation`、`Node`、`Server Home`、`Snapshot`は別moduleが所有しますが、MinecraftServer controllerがlifecycleを協調させます。

## Spec

initial Specは次のcategoryを持ちます。

```yaml
metadata:
  id: survival

spec:
  desiredState: Running

  minecraft:
    type: PAPER
    version: "<explicit-version>"
    eulaAccepted: true

  runtime:
    image: docker.io/itzg/minecraft-server:<explicit-reference>
    memory: 6Gi
    gamePort: 25565
    environment: {}

  nodePolicy:
    mode: OnDemand
    provider: Akamai
    region: <region-id>
    type: <type-id>

  backupPolicy:
    backupBeforeNodeRelease: true
    schedule: null

  automation:
    stopWhenEmptyFor: null
```

exact schemaはimplementationとともにversioned DTOとして定義します。重要なinvariantは次です。

- `TYPE`と`VERSION`を明示する
- `EULA=TRUE`はOperatorのexplicit acceptanceを要求する
- Minecraft-specific environmentだけをallowlistまたはvalidationする
- arbitrary container command、privileged flag、host path mountをOperator Specへ公開しない

## itzg runtime contract

v1の唯一のruntime implementationはitzg/minecraft-serverです。

Node AgentはSpecから次をmaterializeします。

- container image reference
- itzg environment variables
- `Server Home/data`から`/data`へのmount
- `RCON_PASSWORD_FILE`
- game port mapping
- memory/resource settings
- healthcheck
- Quadlet unit

applied manifestにはdesired generation、image reference、resolved image digest、effective environment、port、resource settingを記録します。

## Runtime configuration and Server Home

Runtime configurationはControl Plane databaseだけに閉じ込めません。Node Agentはeffective configurationを`Server Home/manifest.json`へ書き、Server Home backupへ含めます。

これにより各Snapshotは、dataとその時点の起動設定を一緒に保持します。

Control Planeが存在する通常運用ではSpecがcurrent authorityです。Snapshotからのdisaster recoveryではmanifestをrecovery inputとして利用できます。

## Readiness

`RuntimeReady=True`には少なくとも次を要求します。

- desired generationがmanifestへ適用済み
- systemd unitがactive
- Podman containerがrunning
- container healthcheckがhealthy
- local RCON queryが成功

`MinecraftServer Ready=True`には、さらにactive Node allocationがcurrent Fencing Tokenで有効であることを要求します。

## Minecraft control

v1はRCONを使用します。

project operation:

```text
minecraft.observe
minecraft.players.get
minecraft.save.prepare
minecraft.save.resume
minecraft.stop
```

Control PlaneはRCON command stringを送らず、Node Agent adapterがtyped operationを具体的commandへ変換します。

## Update

Spec変更でGenerationが増えます。Controllerはcurrent Server Homeを維持したままnew generationをapplyします。

Minecraft versionやTYPE変更などdata migrationを伴い得るupdateでは、policyによりpre-update backupを作成できます。automatic rollbackはcontainer configurationだけに限定せず、必要ならSnapshot restoreを明示Operationとして行います。

## Non-responsibilities

- Akamai HTTP API
- restic command construction
- generic container scheduling
- Minecraft以外のprogram execution
- arbitrary RCON console proxy
