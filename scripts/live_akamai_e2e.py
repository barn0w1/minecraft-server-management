#!/usr/bin/env python3
"""Run the billable two-generation Akamai production acceptance test.

This script talks only to an already-running production-configured control plane.
It never receives Akamai, TLS, R2, or restic credentials. The control plane owns
provider lifecycle and delivers node runtime credentials after mTLS enrollment.
"""

from __future__ import annotations

import argparse
import socket
import sys
import time
import uuid
from pathlib import Path
from typing import Any

import local_e2e as local

CONFIRMATION = "I_UNDERSTAND_THIS_CREATES_BILLABLE_AKAMAI_RESOURCES"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Create one production acceptance Server and exercise two complete Akamai VM "
            "generations. This creates billable resources."
        )
    )
    parser.add_argument(
        "--confirm-billable-akamai-run",
        required=True,
        metavar="PHRASE",
        help=f"must equal {CONFIRMATION}",
    )
    parser.add_argument(
        "--socket",
        type=Path,
        default=Path("/run/mcserver/control-plane.sock"),
        help="control-plane Unix socket",
    )
    parser.add_argument("--firewall-id", required=True, type=int)
    parser.add_argument("--region", default="jp-tyo-3")
    parser.add_argument("--image", default="linode/debian13")
    parser.add_argument("--instance-type", default="g6-nanode-1")
    parser.add_argument(
        "--container-image",
        default="docker.io/itzg/minecraft-server:latest",
    )
    parser.add_argument("--server-type", default="VANILLA")
    parser.add_argument("--minecraft-version", default="LATEST")
    parser.add_argument("--host-port", type=int, default=25565)
    parser.add_argument("--stop-timeout-seconds", type=int, default=60)
    parser.add_argument("--timeout-seconds", type=float, default=1800.0)
    parser.add_argument("--cleanup-timeout-seconds", type=float, default=900.0)
    parser.add_argument("--name", default=None)
    parser.add_argument(
        "--skip-port-check",
        action="store_true",
        help="skip the public Minecraft TCP readiness check",
    )
    parser.add_argument(
        "--leave-resources-on-failure",
        action="store_true",
        help="do not request desired state stopped after a failure",
    )
    parser.add_argument(
        "--accept-eula",
        action="store_true",
        help="explicitly accept the Minecraft EULA for this acceptance server",
    )
    return parser.parse_args()


def validate_args(args: argparse.Namespace) -> None:
    if args.confirm_billable_akamai_run != CONFIRMATION:
        raise ValueError(
            "--confirm-billable-akamai-run must exactly equal " f"{CONFIRMATION}"
        )
    if not args.accept_eula:
        raise ValueError("--accept-eula is required")
    if not 1 <= args.host_port <= 65535:
        raise ValueError("--host-port must be between 1 and 65535")
    if args.firewall_id <= 0:
        raise ValueError("--firewall-id must be positive")
    if args.stop_timeout_seconds <= 0:
        raise ValueError("--stop-timeout-seconds must be positive")
    if args.timeout_seconds <= 0 or args.cleanup_timeout_seconds <= 0:
        raise ValueError("timeouts must be positive")
    for name in ("region", "image", "instance_type"):
        value = str(getattr(args, name))
        if not value.strip() or "\0" in value:
            raise ValueError(f"--{name.replace('_', '-')} must be non-blank")


def set_desired_state(
    client: local.JsonRpcClient, server: dict[str, Any], desired_state: str
) -> dict[str, Any]:
    return client.call(
        "server.set_desired_state",
        {
            "server_name": server["name"],
            "desired_state": desired_state,
            "expected_generation": server["generation"],
        },
    )


def wait_for_remote_running(
    client: local.JsonRpcClient,
    server_name: str,
    previous_instance_id: str | None,
    deadline: float,
) -> tuple[dict[str, Any], dict[str, Any]]:
    last_status: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        status = client.call("server.status", {"server_name": server_name})
        last_status = status
        instance = status.get("active_instance")
        compute = status.get("active_compute")
        if (
            instance is not None
            and instance["id"] != previous_instance_id
            and instance.get("process_running")
            and instance.get("data_prepared_at_ms") is not None
            and compute is not None
            and compute.get("provider") == "akamai"
            and compute.get("provider_instance_id")
            and compute.get("public_ipv4")
            and status.get("agent_connected") is True
        ):
            return instance, compute
        time.sleep(2.0)
    raise TimeoutError(
        "timed out waiting for an authenticated remote agent and running "
        f"Minecraft process; last_status={last_status!r}"
    )


def wait_for_absent_compute(
    client: local.JsonRpcClient, server_name: str, deadline: float
) -> None:
    last_status: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        status = client.call("server.status", {"server_name": server_name})
        last_status = status
        if status.get("active_instance") is None and status.get("active_compute") is None:
            return
        time.sleep(2.0)
    raise TimeoutError(
        "timed out waiting for the remote compute allocation to be deleted; "
        f"last_status={last_status!r}"
    )


def wait_for_tcp(host: str, port: int, deadline: float) -> None:
    last_error: OSError | None = None
    while time.monotonic() < deadline:
        try:
            with socket.create_connection((host, port), timeout=3.0):
                return
        except OSError as error:
            last_error = error
            time.sleep(2.0)
    raise TimeoutError(
        f"timed out waiting for Minecraft TCP at {host}:{port}; "
        f"last_error={last_error}"
    )


def run_generation(
    client: local.JsonRpcClient,
    server: dict[str, Any],
    previous_instance_id: str | None,
    expected_source_snapshot: str | None,
    expected_fencing_token: int,
    host_port: int,
    timeout_seconds: float,
    skip_port_check: bool,
) -> tuple[dict[str, Any], dict[str, Any]]:
    server = set_desired_state(client, server, "running")
    deadline = time.monotonic() + timeout_seconds
    instance, compute = wait_for_remote_running(
        client, str(server["name"]), previous_instance_id, deadline
    )
    if instance.get("source_snapshot_id") != expected_source_snapshot:
        raise local.RpcError(
            "source snapshot mismatch: "
            f"expected={expected_source_snapshot!r}, "
            f"actual={instance.get('source_snapshot_id')!r}"
        )
    if int(instance["fencing_token"]) != expected_fencing_token:
        raise local.RpcError(
            f"unexpected fencing token: expected={expected_fencing_token}, "
            f"actual={instance['fencing_token']}"
        )

    public_ipv4 = str(compute["public_ipv4"])
    print(
        "running "
        f"instance={instance['id']} compute={compute['provider_instance_id']} "
        f"ipv4={public_ipv4} fencing_token={instance['fencing_token']} "
        f"source_snapshot={instance.get('source_snapshot_id')!r}"
    )
    if not skip_port_check:
        wait_for_tcp(public_ipv4, host_port, deadline)
        print(f"Minecraft TCP is accepting connections at {public_ipv4}:{host_port}")

    server = set_desired_state(client, server, "stopped")
    stop_deadline = time.monotonic() + timeout_seconds
    completed = local.wait_for_completed_instance(
        client, str(server["name"]), str(instance["id"]), stop_deadline
    )
    wait_for_absent_compute(client, str(server["name"]), stop_deadline)
    server = client.call("server.get", {"server_name": server["name"]})
    snapshot = completed.get("result_snapshot_id")
    if not snapshot or server.get("current_snapshot_id") != snapshot:
        raise local.RpcError(
            "published snapshot does not match the completed generation: "
            f"server={server.get('current_snapshot_id')!r}, instance={snapshot!r}"
        )
    print(
        f"completed instance={instance['id']} snapshot={snapshot}; "
        "remote VM deleted"
    )
    return server, completed


def cleanup(
    client: local.JsonRpcClient,
    server: dict[str, Any],
    cleanup_timeout_seconds: float,
) -> None:
    try:
        current = client.call("server.get", {"server_name": server["name"]})
        if current["desired_state"] != "stopped":
            current = set_desired_state(client, current, "stopped")
        deadline = time.monotonic() + cleanup_timeout_seconds
        active = local.active_instance(client, str(current["name"]))
        if active is not None:
            local.wait_for_completed_instance(
                client, str(current["name"]), str(active["id"]), deadline
            )
        wait_for_absent_compute(client, str(current["name"]), deadline)
    except (OSError, local.RpcError, TimeoutError) as error:
        print(f"cleanup warning: {error}", file=sys.stderr)


def main() -> int:
    args = parse_args()
    validate_args(args)
    client = local.JsonRpcClient(args.socket.expanduser().resolve(), timeout_seconds=30.0)
    ping = client.call("system.ping")
    if ping.get("status") != "ok":
        raise local.RpcError(f"unexpected control-plane ping response: {ping!r}")

    server: dict[str, Any] | None = None
    succeeded = False
    try:
        server = client.call(
            "server.create",
            {
                "name": args.name or f"live-akamai-e2e-{uuid.uuid4()}",
                "spec": {
                    "compute": {
                        "provider": "akamai",
                        "region": args.region,
                        "instance_type": args.instance_type,
                        "image": args.image,
                        "firewall_id": args.firewall_id,
                    },
                    "process": {
                        "container_image": args.container_image,
                        "server_type": args.server_type,
                        "version": args.minecraft_version,
                        "host_port": args.host_port,
                        "stop_timeout_seconds": args.stop_timeout_seconds,
                        "accept_eula": True,
                        "environment": {},
                    },
                    "data": {"backend": "r2_restic"},
                },
            },
        )
        print(f"created acceptance server={server['id']} name={server['name']}")

        server, first = run_generation(
            client,
            server,
            previous_instance_id=None,
            expected_source_snapshot=None,
            expected_fencing_token=1,
            host_port=args.host_port,
            timeout_seconds=args.timeout_seconds,
            skip_port_check=args.skip_port_check,
        )
        server, second = run_generation(
            client,
            server,
            previous_instance_id=str(first["id"]),
            expected_source_snapshot=str(first["result_snapshot_id"]),
            expected_fencing_token=2,
            host_port=args.host_port,
            timeout_seconds=args.timeout_seconds,
            skip_port_check=args.skip_port_check,
        )
        if int(second["fencing_token"]) <= int(first["fencing_token"]):
            raise local.RpcError("fencing token did not increase")
        final_status = client.call("server.status", {"server_name": server["name"]})
        if final_status.get("active_instance") is not None or final_status.get(
            "active_compute"
        ) is not None:
            raise local.RpcError(f"final resource state is not empty: {final_status!r}")
        server = client.call(
            "server.archive",
            {
                "server_name": server["name"],
                "expected_generation": server["generation"],
            },
        )
        if server.get("archived_at_ms") is None:
            raise local.RpcError(f"acceptance server was not archived: {server!r}")

        print(
            "live Akamai production checkpoint passed: mTLS enrollment, two VM "
            "generations, restore, start, stop, snapshot publication, and provider "
            "cleanup all succeeded"
        )
        print(
            f"archived Server record and R2 repository retained: "
            f"name={server['name']} id={server['id']}"
        )
        succeeded = True
        return 0
    finally:
        if (
            not succeeded
            and server is not None
            and not args.leave_resources_on_failure
        ):
            cleanup(client, server, args.cleanup_timeout_seconds)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("live Akamai E2E interrupted", file=sys.stderr)
        raise SystemExit(130) from None
    except (OSError, local.RpcError, TimeoutError, ValueError) as error:
        print(f"live Akamai E2E failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
