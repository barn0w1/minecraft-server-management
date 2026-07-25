# Module boundaries

## Control Plane modules

```text
interface
  ├─ operator_api
  └─ agent_api
        ↓
application
  ├─ minecraft
  ├─ server_data
  ├─ workload
  └─ node
        ↓
domain models and narrow ports
        ↑
infrastructure
  ├─ sqlite
  ├─ akamai
  ├─ quic
  ├─ pki
  └─ filesystem/secrets
```

- interfaceはdecode、authentication context、response mappingを担当する
- applicationはuse case、transaction boundary、operation intent、orchestrationを担当する
- domainはinvariantとstate transitionを表現し、SQL、HTTP、filesystemへ依存しない
- infrastructureはexternal I/Oを実装する
- runtime wiringだけがconcrete implementationを接続する

## Node Agent modules

```text
agent_core
  ├─ enrollment
  ├─ identity
  ├─ connection
  ├─ heartbeat
  ├─ rpc_dispatch
  └─ task supervision

capabilities
  ├─ node
  ├─ workload
  ├─ server_data
  └─ minecraft

adapters
  ├─ linux/systemd
  ├─ podman/quadlet
  ├─ restic/filesystem
  └─ management_protocol/rcon
```

`agent_core`はMinecraft、restic、Podmanのdomain ruleを知りません。RPC methodを認証済みconnection contextとともに対応capabilityへdispatchします。

## Ownership rules

- moduleは自身が所有するtableだけを更新する
- sibling moduleのinternal typeやtableへ直接依存しない
- cross-domain actionはapplication-level commandまたはquery contractを通す
- external adapter typeをdomain public APIへ漏らさない
- shared crateはwire contractや本当に共有されるprimitiveだけに限定する

## Allowed dependencies

```text
minecraft application → server_data, workload, node ports
server_data application → Node Agent data execution port
workload application → node availability/query port
node application → Akamai adapter, Agent node port
```

禁止例:

- Akamai adapterがMinecraftServerを読む
- Minecraft moduleがLinode APIを直接呼ぶ
- Server Data moduleがRCONでsaveを実行する
- Workload moduleがMinecraft player数を解釈する
- RPC handlerがdatabase transactionとlifecycle policyを直接組み立てる
