# Client JSON-RPC API

## Transport and framing

The client API uses JSON-RPC 2.0 over a Unix stream socket. JSON-RPC itself does not define stream framing, so this project uses one UTF-8 JSON value per line.

- Default path: `/run/mcserver/control-plane.sock`
- Default mode: `0660`
- Maximum frame size: 1 MiB by default
- A line may contain one request object or one batch array
- Notifications produce no response
- Responses are also one JSON value per line

## Methods

### `system.ping`

No params. Returns process status and package version.

### `server.create`

Creates a durable `Server` in desired state `stopped`. The data repository is opaque to the control plane.

### `server.get`

Returns one server by UUID.

### `server.list`

Returns all servers, ordered by name and UUID.

### `server.set_desired_state`

Sets `running` or `stopped`. An optional `expected_generation` provides optimistic concurrency control. A successful state change increments `generation`; setting the same value is idempotent and does not increment it.
