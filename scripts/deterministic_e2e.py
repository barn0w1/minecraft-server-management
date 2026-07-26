#!/usr/bin/env python3
"""Run the complete local lifecycle with real Rust processes and fake external tools."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import uuid


def free_tcp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def require_executable(path: Path, description: str) -> Path:
    resolved = path.resolve()
    if not resolved.is_file() or not os.access(resolved, os.X_OK):
        raise RuntimeError(f"{description} is not executable: {resolved}")
    return resolved


def wait_for_socket(process: subprocess.Popen[bytes], path: Path, deadline: float) -> None:
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(
                f"control-plane exited before creating its socket with status {process.returncode}"
            )
        if path.exists():
            return
        time.sleep(0.05)
    raise TimeoutError(f"timed out waiting for control-plane socket: {path}")


def load_fake_containers(runtime: Path) -> dict[str, object]:
    state = runtime / "containers.json"
    if not state.exists():
        return {}
    value = json.loads(state.read_text())
    containers = value.get("containers", {})
    if not isinstance(containers, dict):
        raise RuntimeError("fake Podman state has invalid containers data")
    return containers


def seed_orphans(
    podman: Path,
    runtime: Path,
    node_root: Path,
    local_scope: str,
) -> tuple[str, Path, subprocess.Popen[bytes]]:
    orphan_instance = str(uuid.uuid4())
    orphan_compute = str(uuid.uuid4())
    environment = os.environ.copy()
    environment["MCSERVER_FAKE_RUNTIME_DIRECTORY"] = str(runtime)
    subprocess.run(
        [
            str(podman),
            "create",
            "--replace",
            "--name",
            f"mcserver-{orphan_instance}",
            "--label",
            "io.mcserver.managed=true",
            "--label",
            f"io.mcserver.local-scope={local_scope}",
            "--label",
            f"io.mcserver.server-id={uuid.uuid4()}",
            "--label",
            f"io.mcserver.server-instance-id={orphan_instance}",
            "--label",
            f"io.mcserver.compute-instance-id={orphan_compute}",
            "--volume",
            f"{node_root / orphan_compute / 'data'}:/data:Z",
            "--publish",
            "65534:25565/tcp",
            "--",
            "fake/minecraft-server:latest",
        ],
        check=True,
        env=environment,
    )
    orphan_directory = node_root / orphan_compute
    orphan_directory.mkdir(parents=True)
    (orphan_directory / "orphan.txt").write_text("orphan\n")
    process_environment = os.environ.copy()
    process_environment.update(
        {
            "MCSERVER_NODE_AGENT_COMPUTE_INSTANCE_ID": orphan_compute,
            "MCSERVER_NODE_AGENT_LOCAL_SCOPE": local_scope,
        }
    )
    orphan_process = subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(300)"],
        env=process_environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return orphan_instance, orphan_directory, orphan_process


def terminate_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    process.send_signal(signal.SIGINT)
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--control-plane-binary",
        type=Path,
        default=Path("target/debug/mcserver-control-plane"),
    )
    parser.add_argument(
        "--node-agent-binary",
        type=Path,
        default=Path("target/debug/mcserver-node-agent"),
    )
    parser.add_argument(
        "--work-directory",
        type=Path,
        default=None,
        help="fixed work directory; by default a temporary directory is used",
    )
    parser.add_argument(
        "--keep-work-directory",
        action="store_true",
        help="retain the temporary database, fake state, and logs after success",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repository_root = Path(__file__).resolve().parent.parent
    control_plane = require_executable(args.control_plane_binary, "control-plane binary")
    node_agent = require_executable(args.node_agent_binary, "node-agent binary")
    fake_podman = require_executable(
        repository_root / "scripts/fakes/podman.py", "fake Podman"
    )
    fake_restic = require_executable(
        repository_root / "scripts/fakes/restic.py", "fake restic"
    )
    local_e2e = repository_root / "scripts/local_e2e.py"

    temporary = args.work_directory is None
    if temporary:
        work_directory = Path(
            tempfile.mkdtemp(prefix="mcserver-deterministic-e2e-")
        )
    else:
        work_directory = args.work_directory.resolve()
        if work_directory.exists():
            shutil.rmtree(work_directory)
        work_directory.mkdir(parents=True)

    socket_path = work_directory / "control-plane.sock"
    database_path = work_directory / "control-plane.db"
    node_root = work_directory / "local-agents"
    runtime = work_directory / "fake-runtime"
    restic_repository = work_directory / "restic-repository"
    control_plane_log = work_directory / "control-plane.log"
    local_scope = f"deterministic-{uuid.uuid4()}"
    agent_port = free_tcp_port()
    minecraft_port = free_tcp_port()

    environment = os.environ.copy()
    environment.update(
        {
            "MCSERVER_CONTROL_PLANE_SOCKET": str(socket_path),
            "MCSERVER_CONTROL_PLANE_DATABASE_URL": (
                f"sqlite://{database_path}?mode=rwc"
            ),
            "MCSERVER_CONTROL_PLANE_AGENT_LISTEN_ADDRESS": (
                f"127.0.0.1:{agent_port}"
            ),
            "MCSERVER_CONTROL_PLANE_NODE_AGENT_BINARY": str(node_agent),
            "MCSERVER_CONTROL_PLANE_NODE_AGENT_ROOT": str(node_root),
            "MCSERVER_CONTROL_PLANE_PODMAN_BINARY": str(fake_podman),
            "MCSERVER_CONTROL_PLANE_LOCAL_SCOPE": local_scope,
            "MCSERVER_CONTROL_PLANE_REAP_ORPHANS_ON_START": "true",
            "MCSERVER_CONTROL_PLANE_RECONCILE_INTERVAL_SECONDS": "1",
            "MCSERVER_CONTROL_PLANE_RECONCILE_RETRY_SECONDS": "1",
            "MCSERVER_CONTROL_PLANE_AGENT_COMMAND_TIMEOUT_SECONDS": "30",
            "MCSERVER_CONTROL_PLANE_LOCAL_CONTROL_TIMEOUT_SECONDS": "5",
            "MCSERVER_CONTROL_PLANE_LOCAL_PROCESS_STOP_TIMEOUT_SECONDS": "3",
            "MCSERVER_CONTROL_PLANE_SHUTDOWN_TIMEOUT_SECONDS": "5",
            "MCSERVER_NODE_AGENT_PODMAN_BINARY": str(fake_podman),
            "MCSERVER_NODE_AGENT_RESTIC_BINARY": str(fake_restic),
            "MCSERVER_NODE_AGENT_RESTIC_RETRY_LOCK_SECONDS": "1",
            "MCSERVER_NODE_AGENT_RECONNECT_MIN_SECONDS": "1",
            "MCSERVER_NODE_AGENT_RECONNECT_MAX_SECONDS": "2",
            "MCSERVER_FAKE_RUNTIME_DIRECTORY": str(runtime),
            "MCSERVER_FAKE_PODMAN_FAIL_ONCE": "start",
            "MCSERVER_FAKE_RESTIC_FAIL_ONCE": "backup",
            "RESTIC_PASSWORD": "deterministic-test-only",
            "RUST_LOG": (
                "mcserver_control_plane=info,mcserver_node_agent=info"
            ),
        }
    )

    process: subprocess.Popen[bytes] | None = None
    orphan_process: subprocess.Popen[bytes] | None = None
    succeeded = False
    try:
        _, orphan_directory, orphan_process = seed_orphans(
            fake_podman, runtime, node_root, local_scope
        )
        with control_plane_log.open("wb") as log:
            process = subprocess.Popen(
                [str(control_plane)],
                cwd=repository_root,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            wait_for_socket(process, socket_path, time.monotonic() + 15)

            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                if (
                    not orphan_directory.exists()
                    and not load_fake_containers(runtime)
                    and orphan_process.poll() is not None
                ):
                    break
                time.sleep(0.05)
            else:
                raise RuntimeError("startup orphan reaper did not remove seeded resources")

            common_e2e_arguments = [
                sys.executable,
                str(local_e2e),
                "--socket",
                str(socket_path),
                "--repository",
                str(restic_repository),
                "--stop-timeout-seconds",
                "1",
                "--podman-binary",
                str(fake_podman),
                "--restic-binary",
                str(fake_restic),
                "--local-scope",
                local_scope,
            ]
            subprocess.run(
                [
                    *common_e2e_arguments,
                    "--name",
                    f"deterministic-success-{uuid.uuid4()}",
                    "--host-port",
                    str(minecraft_port),
                    "--timeout-seconds",
                    "60",
                    "--cleanup-timeout-seconds",
                    "10",
                    "--skip-port-check",
                ],
                cwd=repository_root,
                env=environment,
                check=True,
            )

            expected_failure = subprocess.run(
                [
                    *common_e2e_arguments,
                    "--name",
                    f"deterministic-failure-{uuid.uuid4()}",
                    "--host-port",
                    str(free_tcp_port()),
                    "--timeout-seconds",
                    "4",
                    "--cleanup-timeout-seconds",
                    "15",
                ],
                cwd=repository_root,
                env=environment,
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
            )
            if expected_failure.returncode == 0:
                raise RuntimeError(
                    "the expected Minecraft readiness failure unexpectedly succeeded"
                )
            if "timed out waiting for Minecraft TCP port" not in expected_failure.stdout:
                raise RuntimeError(
                    "the expected failure scenario failed for an unexpected reason:\n"
                    + expected_failure.stdout
                )

            remaining = load_fake_containers(runtime)
            if remaining:
                raise RuntimeError(
                    f"managed fake containers remain after E2E: {sorted(remaining)}"
                )
            remaining_state = (
                sorted(path.name for path in node_root.iterdir())
                if node_root.exists()
                else []
            )
            if remaining_state:
                raise RuntimeError(
                    f"local compute state remains after E2E: {remaining_state}"
                )
            if process.poll() is not None:
                raise RuntimeError(
                    f"control-plane exited unexpectedly with status {process.returncode}"
                )
        succeeded = True
        print(
            "deterministic local E2E passed: startup orphan reaping, transient "
            "failure recovery, two complete generations, and failed-run cleanup "
            "succeeded"
        )
        return 0
    except (OSError, RuntimeError, TimeoutError, subprocess.CalledProcessError) as error:
        print(f"deterministic local E2E failed: {error}", file=sys.stderr)
        print(f"artifacts retained at {work_directory}", file=sys.stderr)
        return 1
    finally:
        if process is not None:
            terminate_process(process)
        if orphan_process is not None:
            terminate_process(orphan_process)
        if succeeded and temporary and not args.keep_work_directory:
            shutil.rmtree(work_directory, ignore_errors=True)
        elif succeeded:
            print(f"artifacts retained at {work_directory}")


if __name__ == "__main__":
    raise SystemExit(main())
