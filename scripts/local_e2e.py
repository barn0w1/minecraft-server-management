#!/usr/bin/env python3
"""Exercise the complete local control-plane vertical slice.

The script expects a running mcserver-control-plane. It creates one Server,
starts and stops it twice, and verifies that the second ServerInstance restores
from the snapshot published by the first one.
"""

from __future__ import annotations

import argparse
import errno
import json
import os
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Callable


class RpcError(RuntimeError):
    pass


class JsonRpcClient:
    def __init__(self, socket_path: Path, timeout_seconds: float) -> None:
        self.socket_path = socket_path
        self.timeout_seconds = timeout_seconds
        self.next_id = 1

    def call(self, method: str, params: dict[str, Any] | None = None) -> Any:
        request_id = self.next_id
        self.next_id += 1
        request: dict[str, Any] = {
            "jsonrpc": "2.0",
            "method": method,
            "id": request_id,
        }
        if params is not None:
            request["params"] = params

        payload = json.dumps(request, separators=(",", ":")).encode() + b"\n"
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(self.timeout_seconds)
            connection.connect(str(self.socket_path))
            connection.sendall(payload)
            response_bytes = read_line(connection, maximum=1024 * 1024)

        response = json.loads(response_bytes)
        if response.get("jsonrpc") != "2.0":
            raise RpcError(f"invalid JSON-RPC response version: {response!r}")
        if response.get("id") != request_id:
            raise RpcError(
                f"response id mismatch: expected {request_id}, got {response.get('id')!r}"
            )
        error = response.get("error")
        if error is not None:
            detail = error.get("data")
            raise RpcError(
                f"{method} failed: {error.get('code')} {error.get('message')}; "
                f"detail={detail!r}"
            )
        if "result" not in response:
            raise RpcError(f"{method} returned neither result nor error")
        return response["result"]


def read_line(connection: socket.socket, maximum: int) -> bytes:
    chunks: list[bytes] = []
    size = 0
    while True:
        chunk = connection.recv(min(64 * 1024, maximum - size + 1))
        if not chunk:
            raise RpcError("control plane disconnected before returning a response")
        newline = chunk.find(b"\n")
        if newline >= 0:
            size += newline
            if size > maximum:
                raise RpcError(f"response exceeds {maximum} bytes")
            chunks.append(chunk[:newline])
            return b"".join(chunks)
        chunks.append(chunk)
        size += len(chunk)
        if size > maximum:
            raise RpcError(f"response exceeds {maximum} bytes")


def ensure_restic_repository(repository: Path, restic_binary: str) -> None:
    restic = shutil.which(restic_binary)
    if restic is None:
        raise RpcError(f"restic binary is not available: {restic_binary}")
    if repository.exists():
        result = subprocess.run(
            [
                restic,
                "--insecure-no-password",
                "--repo",
                str(repository),
                "cat",
                "config",
            ],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        if result.returncode != 0:
            raise RpcError(
                "the requested repository path already exists but is not an accessible "
                f"passwordless restic repository: {result.stderr.strip()}"
            )
        return

    repository.parent.mkdir(parents=True, exist_ok=True)
    print(f"repository will be initialized automatically={repository}")


def wait_until(
    description: str,
    deadline: float,
    operation: Callable[[], Any | None],
    interval_seconds: float = 1.0,
) -> Any:
    last_value: Any = None
    while time.monotonic() < deadline:
        value = operation()
        if value is not None:
            return value
        last_value = value
        time.sleep(interval_seconds)
    raise TimeoutError(f"timed out waiting for {description}; last={last_value!r}")


def list_instances(client: JsonRpcClient, server_name: str) -> list[dict[str, Any]]:
    result = client.call("server_instance.list", {"server_name": server_name})
    return result["server_instances"]


def active_instance(client: JsonRpcClient, server_name: str) -> dict[str, Any] | None:
    instances = list_instances(client, server_name)
    active = [item for item in instances if item["terminated_at_ms"] is None]
    if len(active) > 1:
        raise RpcError(f"more than one active ServerInstance exists: {active!r}")
    return active[0] if active else None


def wait_for_running_instance(
    client: JsonRpcClient,
    server_name: str,
    previous_instance_id: str | None,
    deadline: float,
) -> dict[str, Any]:
    last_error: str | None = None

    def inspect() -> dict[str, Any] | None:
        nonlocal last_error
        instance = active_instance(client, server_name)
        if instance is None or instance["id"] == previous_instance_id:
            return None
        last_error = instance.get("last_error")
        if instance["process_running"] and instance["data_prepared_at_ms"] is not None:
            return instance
        return None

    try:
        return wait_until("a running ServerInstance", deadline, inspect)
    except TimeoutError as error:
        raise TimeoutError(f"{error}; reconciler last_error={last_error!r}") from error


def wait_for_completed_instance(
    client: JsonRpcClient,
    server_name: str,
    instance_id: str,
    deadline: float,
) -> dict[str, Any]:
    last_error: str | None = None

    def inspect() -> dict[str, Any] | None:
        nonlocal last_error
        for instance in list_instances(client, server_name):
            if instance["id"] != instance_id:
                continue
            last_error = instance.get("last_error")
            if instance["terminated_at_ms"] is None:
                return None
            if instance["terminal_result"] != "completed":
                raise RpcError(f"ServerInstance terminated unsuccessfully: {instance!r}")
            if not instance.get("result_snapshot_id"):
                raise RpcError(f"completed ServerInstance has no snapshot: {instance!r}")
            return instance
        return None

    try:
        return wait_until("a completed ServerInstance", deadline, inspect)
    except TimeoutError as error:
        raise TimeoutError(f"{error}; reconciler last_error={last_error!r}") from error


def wait_for_tcp_port(host: str, port: int, deadline: float) -> None:
    while time.monotonic() < deadline:
        try:
            with socket.create_connection((host, port), timeout=1.0):
                return
        except OSError:
            time.sleep(1.0)
    raise TimeoutError(f"timed out waiting for Minecraft TCP port {host}:{port}")


def tcp_publish_port_is_available(port: int) -> bool:
    addresses = [
        (socket.AF_INET, ("0.0.0.0", port)),
        (socket.AF_INET6, ("::", port, 0, 0)),
    ]
    checked = False
    for family, address in addresses:
        try:
            with socket.socket(family, socket.SOCK_STREAM) as probe:
                probe.bind(address)
            checked = True
        except OSError as error:
            if error.errno in {errno.EAFNOSUPPORT, errno.EADDRNOTAVAIL}:
                continue
            return False
    return checked


def force_remove_test_containers(
    podman_binary: str,
    local_scope: str,
    server_id: str,
) -> None:
    podman = shutil.which(podman_binary)
    if podman is None:
        print(
            f"cleanup warning: Podman binary is not available: {podman_binary}",
            file=sys.stderr,
        )
        return
    result = subprocess.run(
        [
            podman,
            "ps",
            "--all",
            "--quiet",
            "--filter",
            "label=io.mcserver.managed=true",
            "--filter",
            f"label=io.mcserver.local-scope={local_scope}",
            "--filter",
            f"label=io.mcserver.server-id={server_id}",
        ],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        print(
            f"cleanup warning: failed to list test containers: {result.stderr.strip()}",
            file=sys.stderr,
        )
        return
    container_ids = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    if not container_ids:
        return
    removed = subprocess.run(
        [podman, "rm", "--force", "--ignore", *container_ids],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    if removed.returncode != 0:
        print(
            f"cleanup warning: failed to remove test containers: {removed.stderr.strip()}",
            file=sys.stderr,
        )


def cleanup_failed_run(
    client: JsonRpcClient,
    server: dict[str, Any],
    cleanup_deadline: float,
    podman_binary: str,
    local_scope: str,
) -> None:
    server_id = server["id"]
    server_name = server["name"]
    try:
        current = client.call("server.get", {"server_name": server_name})
        if current["desired_state"] != "stopped":
            current = set_desired_state(client, current, "stopped")
        instance = active_instance(client, server_name)
        if instance is not None:
            wait_for_completed_instance(
                client,
                server_name,
                instance["id"],
                cleanup_deadline,
            )
    except (OSError, RpcError, TimeoutError) as error:
        print(
            f"cleanup warning: graceful test cleanup failed: {error}",
            file=sys.stderr,
        )
    finally:
        force_remove_test_containers(podman_binary, local_scope, server_id)


def set_desired_state(
    client: JsonRpcClient, server: dict[str, Any], desired_state: str
) -> dict[str, Any]:
    return client.call(
        "server.set_desired_state",
        {
            "server_name": server["name"],
            "desired_state": desired_state,
            "expected_generation": server["generation"],
        },
    )


def run_cycle(
    client: JsonRpcClient,
    server: dict[str, Any],
    previous_instance_id: str | None,
    expected_source_snapshot: str | None,
    host: str,
    port: int,
    deadline: float,
    wait_for_port: bool,
) -> tuple[dict[str, Any], dict[str, Any]]:
    server = set_desired_state(client, server, "running")
    instance = wait_for_running_instance(
        client, server["name"], previous_instance_id, deadline
    )
    if instance.get("source_snapshot_id") != expected_source_snapshot:
        raise RpcError(
            "ServerInstance source snapshot mismatch: "
            f"expected {expected_source_snapshot!r}, "
            f"got {instance.get('source_snapshot_id')!r}"
        )
    print(
        f"running instance={instance['id']} fencing_token={instance['fencing_token']} "
        f"source_snapshot={instance.get('source_snapshot_id')!r}"
    )
    if wait_for_port:
        wait_for_tcp_port(host, port, deadline)
        print(f"Minecraft TCP port is accepting connections at {host}:{port}")

    server = set_desired_state(client, server, "stopped")
    completed = wait_for_completed_instance(
        client, server["name"], instance["id"], deadline
    )
    server = client.call("server.get", {"server_name": server["name"]})
    snapshot_id = completed["result_snapshot_id"]
    if server.get("current_snapshot_id") != snapshot_id:
        raise RpcError(
            "Server current snapshot does not match the completed instance: "
            f"server={server.get('current_snapshot_id')!r}, instance={snapshot_id!r}"
        )
    print(f"completed instance={instance['id']} snapshot={snapshot_id}")
    return server, completed


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--socket",
        type=Path,
        default=Path("var/control-plane.sock"),
        help="control-plane Unix socket path",
    )
    parser.add_argument(
        "--repository",
        type=Path,
        default=Path("var/restic-repository"),
        help="local restic repository path",
    )
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--host-port", type=int, default=25565)
    parser.add_argument("--stop-timeout-seconds", type=int, default=60)
    parser.add_argument("--timeout-seconds", type=float, default=900.0)
    parser.add_argument("--cleanup-timeout-seconds", type=float, default=120.0)
    parser.add_argument(
        "--podman-binary",
        default=os.environ.get("MCSERVER_CONTROL_PLANE_PODMAN_BINARY", "podman"),
    )
    parser.add_argument(
        "--restic-binary",
        default=os.environ.get("MCSERVER_NODE_AGENT_RESTIC_BINARY", "restic"),
    )
    parser.add_argument(
        "--local-scope",
        default=os.environ.get("MCSERVER_CONTROL_PLANE_LOCAL_SCOPE", "default"),
    )
    parser.add_argument(
        "--name",
        default=None,
        help="Server name; defaults to a unique local-e2e name",
    )
    parser.add_argument(
        "--skip-port-check",
        action="store_true",
        help="only verify the Podman process observation, not Minecraft readiness",
    )
    parser.add_argument(
        "--leave-resources-on-failure",
        action="store_true",
        help="do not request stop or remove managed test containers after a failure",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not 1 <= args.host_port <= 65535:
        raise ValueError("--host-port must be between 1 and 65535")
    if args.stop_timeout_seconds <= 0:
        raise ValueError("--stop-timeout-seconds must be greater than zero")
    if args.timeout_seconds <= 0:
        raise ValueError("--timeout-seconds must be greater than zero")
    if args.cleanup_timeout_seconds <= 0:
        raise ValueError("--cleanup-timeout-seconds must be greater than zero")
    if not args.local_scope.strip() or "\0" in args.local_scope:
        raise ValueError("--local-scope must be non-blank and contain no NUL byte")
    if len(args.local_scope) > 128:
        raise ValueError("--local-scope must be no longer than 128 characters")
    if not all(character.isascii() and (character.isalnum() or character in "._-") for character in args.local_scope):
        raise ValueError(
            "--local-scope must contain only ASCII letters, digits, dot, underscore, or hyphen"
        )
    if not tcp_publish_port_is_available(args.host_port):
        raise RpcError(
            f"host publish port {args.host_port} is unavailable; "
            "stop the existing process or choose another --host-port"
        )

    socket_path = args.socket.expanduser().resolve()
    repository = args.repository.expanduser().resolve()
    ensure_restic_repository(repository, args.restic_binary)
    name = args.name or f"local-e2e-{int(time.time())}"
    deadline = time.monotonic() + args.timeout_seconds
    client = JsonRpcClient(socket_path, timeout_seconds=30.0)
    server: dict[str, Any] | None = None
    succeeded = False

    try:
        ping = client.call("system.ping")
        print(f"control plane status={ping['status']} version={ping['version']}")
        server = client.call(
            "server.create",
            {
                "name": name,
                "spec": {
                    "compute": {"provider": "local"},
                    "process": {
                        "container_image": "docker.io/itzg/minecraft-server:latest",
                        "server_type": "VANILLA",
                        "version": "LATEST",
                        "host_port": args.host_port,
                        "stop_timeout_seconds": args.stop_timeout_seconds,
                        "accept_eula": True,
                        "environment": {},
                    },
                    "data": {
                        "backend": "local_restic",
                        "repository": str(repository),
                    },
                },
            },
        )
        print(f"created server={server['id']} name={server['name']}")

        server, first = run_cycle(
            client,
            server,
            previous_instance_id=None,
            expected_source_snapshot=None,
            host=args.host,
            port=args.host_port,
            deadline=deadline,
            wait_for_port=not args.skip_port_check,
        )
        first_snapshot = first["result_snapshot_id"]

        server, second = run_cycle(
            client,
            server,
            previous_instance_id=first["id"],
            expected_source_snapshot=first_snapshot,
            host=args.host,
            port=args.host_port,
            deadline=deadline,
            wait_for_port=not args.skip_port_check,
        )
        if second["fencing_token"] <= first["fencing_token"]:
            raise RpcError("fencing token did not increase between instances")

        print(
            "local vertical slice passed: create, restore, start, stop, snapshot, "
            "publish, and second-generation restore all succeeded"
        )
        succeeded = True
        return 0
    finally:
        if (
            not succeeded
            and server is not None
            and not args.leave_resources_on_failure
        ):
            cleanup_failed_run(
                client,
                server,
                time.monotonic() + args.cleanup_timeout_seconds,
                args.podman_binary,
                args.local_scope,
            )


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("local E2E interrupted", file=sys.stderr)
        raise SystemExit(130) from None
    except (OSError, RpcError, TimeoutError, ValueError) as error:
        print(f"local E2E failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
