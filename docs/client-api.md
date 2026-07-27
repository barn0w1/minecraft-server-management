# Client JSON-RPC API

## Transport

The client API uses JSON-RPC 2.0 over a Unix stream socket.

- default path: `/run/mcserver/control-plane.sock`
- default mode: `0660`
- framing: one UTF-8 JSON value followed by `\n`
- default maximum frame: 1 MiB
- one frame can be a request object or batch array
- notifications produce no response

All fields ending in `_at_ms` are non-negative Unix timestamps in milliseconds.

## Methods

### `system.ping`

No params.

### `server.create`

Creates a durable Server in desired state `stopped`.

Example:

```json
{
  "jsonrpc": "2.0",
  "method": "server.create",
  "params": {
    "name": "community",
    "spec": {
      "compute": { "provider": "local" },
      "process": {
        "container_image": "docker.io/itzg/minecraft-server:latest",
        "server_type": "VANILLA",
        "version": "LATEST",
        "host_port": 25565,
        "stop_timeout_seconds": 60,
        "accept_eula": true,
        "environment": {}
      },
      "data": {
        "repository": "/absolute/path/to/restic-repository"
      }
    }
  },
  "id": 1
}
```

The following environment keys are system-owned and rejected in `environment`:

- `EULA`
- `TYPE`
- `VERSION`
- `SKIP_SERVER_PROPERTIES`

The system sets `SKIP_SERVER_PROPERTIES=TRUE`; files under `/data`, including `server.properties`, remain outside the control-plane configuration model.

For an Akamai-backed Server, `compute` is provider-tagged:

```json
{
  "provider": "akamai",
  "region": "jp-tyo-3",
  "instance_type": "g6-nanode-1",
  "image": "linode/debian13",
  "firewall_id": 123
}
```

`region`, `instance_type`, and `image` are passed to the Linode create API. `firewall_id` is optional. Provider credentials and remote-agent bootstrap settings are control-plane configuration, never client API fields.

### `server.get`

Params: `server_id` UUID.

### `server.list`

No params. Returns Servers ordered by name and UUID.

### `server.status`

Params: `server_id` UUID. Returns an aggregate read-only view containing:

- the durable Server
- the active ServerInstance, when present
- the active ComputeInstance, without its connection token
- whether that ComputeInstance currently has a registered agent session

This is a projection of existing resources, not another persisted lifecycle resource.

### `server.set_desired_state`

Params:

- `server_id`
- `desired_state`: `running` or `stopped`
- optional `expected_generation`

A successful change increments `generation`. Setting the same state is idempotent. The response confirms the durable desired-state update, not completion of external operations.

### `server_instance.get`

Params: `server_instance_id` UUID. Read-only.

### `server_instance.list`

Params: `server_id` UUID. Returns complete history newest first. At most one item has `terminated_at_ms = null`.

Useful observed fields include:

- `source_snapshot_id`
- `data_prepared_at_ms`
- `process_running`
- `process_observed_at_ms`
- `result_snapshot_id`
- `last_error`
- `stop_requested_at_ms`
- `terminated_at_ms`
- `terminal_result`


## Operator CLI

`mcserverctl` uses the same Unix-socket JSON-RPC API:

```text
mcserverctl [--socket PATH] ping
mcserverctl [--socket PATH] server list
mcserverctl [--socket PATH] server get SERVER_ID
mcserverctl [--socket PATH] server status SERVER_ID
mcserverctl [--socket PATH] server instances SERVER_ID
mcserverctl [--socket PATH] server start SERVER_ID
mcserverctl [--socket PATH] server stop SERVER_ID
mcserverctl [--socket PATH] server create --name NAME --repository REPOSITORY --accept-eula [--compute local|akamai] ...
```

For Akamai creation, use `--compute akamai` with `--akamai-region`, `--akamai-type`, `--akamai-image`, and optional `--akamai-firewall-id`.

Start and stop first read the current generation and send an optimistic desired-state update. Mutations still have the same JSON-RPC semantics as direct clients.
