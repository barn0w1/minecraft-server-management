#!/usr/bin/env python3
"""Exercise the remote TLS agent and Akamai provider without creating billable VMs."""

from __future__ import annotations

import argparse
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from typing import Any

AKAMAI_REGION = "jp-tyo-3"
AKAMAI_IMAGE = "linode/debian13"
R2_ACCOUNT_ID = "0123456789abcdef0123456789abcdef"
R2_BUCKET = "mcserver-remote-e2e"
R2_PARENT_ACCESS_KEY_ID = "remote-e2e-parent-access-key"


def load_local_e2e(repository_root: Path) -> Any:
    path = repository_root / "scripts/local_e2e.py"
    spec = importlib.util.spec_from_file_location("mcserver_local_e2e", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def free_tcp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def require_executable(path: Path, description: str) -> Path:
    resolved = path.resolve()
    if not resolved.is_file() or not os.access(resolved, os.X_OK):
        raise RuntimeError(f"{description} is not executable: {resolved}")
    return resolved


def openssl_executable(value: str) -> Path:
    resolved = shutil.which(value)
    if resolved is None:
        raise argparse.ArgumentTypeError(
            f"OpenSSL executable was not found: {value!r}; install OpenSSL or pass "
            "--openssl-binary PATH"
        )
    return Path(resolved).resolve()


class FakeAkamaiState:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.next_id = 1000
        self.instances: dict[int, dict[str, Any]] = {}
        self.requests: list[dict[str, Any]] = []
        self.deleted: list[int] = []
        self.orphan_ids: set[int] = set()
        self.rate_limit_injected = False
        self.lost_create_response_injected = False
        self.lost_delete_response_injected = False
        self.image_preflight_checks = 0
        self.region_preflight_checks = 0
        self.r2_credential_requests: list[dict[str, Any]] = []

    def list_by_label(self, label: str | None) -> list[dict[str, Any]]:
        with self.lock:
            values = list(self.instances.values())
            if label is not None:
                values = [value for value in values if value.get("label") == label]
            return [dict(value) for value in values]

    def seed_orphan(self, scope: str) -> int:
        with self.lock:
            instance_id = self.next_id
            self.next_id += 1
            self.instances[instance_id] = {
                "id": instance_id,
                "label": f"mcserver-{uuid.uuid4()}",
                "status": "running",
                "ipv4": ["203.0.113.9"],
                "tags": ["mcserver-managed", f"mcserver-scope-{scope}"],
                "region": AKAMAI_REGION,
                "type": "g6-nanode-1",
                "image": AKAMAI_IMAGE,
                "has_user_data": True,
            }
            self.orphan_ids.add(instance_id)
            return instance_id

    def should_rate_limit_create(self) -> bool:
        with self.lock:
            if self.rate_limit_injected:
                return False
            self.rate_limit_injected = True
            return True

    def create(self, request: dict[str, Any]) -> tuple[dict[str, Any], bool]:
        with self.lock:
            instance_id = self.next_id
            self.next_id += 1
            value = {
                "id": instance_id,
                "label": request["label"],
                "status": "running",
                "ipv4": [f"203.0.113.{10 + len(self.instances)}"],
                "tags": request.get("tags", []),
                "region": request["region"],
                "type": request["type"],
                "image": request["image"],
                "has_user_data": True,
            }
            self.instances[instance_id] = value
            self.requests.append(request)
            lose_response = not self.lost_create_response_injected
            self.lost_create_response_injected = True
            return dict(value), lose_response

    def get(self, instance_id: int) -> dict[str, Any] | None:
        with self.lock:
            value = self.instances.get(instance_id)
            return None if value is None else dict(value)

    def delete(self, instance_id: int) -> tuple[bool, bool]:
        with self.lock:
            existed = self.instances.pop(instance_id, None) is not None
            lose_response = (
                existed
                and instance_id not in self.orphan_ids
                and not self.lost_delete_response_injected
            )
            if lose_response:
                self.lost_delete_response_injected = True
            if existed:
                self.deleted.append(instance_id)
            return existed, lose_response

    def issue_r2_credentials(self, request: dict[str, Any]) -> dict[str, str]:
        with self.lock:
            self.r2_credential_requests.append(dict(request))
            generation = len(self.r2_credential_requests)
        return {
            "accessKeyId": f"temporary-access-{generation}",
            "secretAccessKey": f"temporary-secret-{generation}",
            "sessionToken": f"temporary-session-{generation}",
        }


class FakeAkamaiHandler(BaseHTTPRequestHandler):
    server: "FakeAkamaiServer"

    def do_GET(self) -> None:  # noqa: N802
        if self.path == f"/v4/images/{AKAMAI_IMAGE}":
            with self.server.state.lock:
                self.server.state.image_preflight_checks += 1
            self.send_json(
                200,
                {
                    "id": AKAMAI_IMAGE,
                    "label": "Debian 13",
                    "status": "available",
                    "deprecated": False,
                    "capabilities": ["cloud-init"],
                },
            )
            return
        if self.path == f"/v4/regions/{AKAMAI_REGION}":
            with self.server.state.lock:
                self.server.state.region_preflight_checks += 1
            self.send_json(
                200,
                {
                    "id": AKAMAI_REGION,
                    "label": "Tokyo 3, JP",
                    "capabilities": ["Linodes", "Metadata"],
                },
            )
            return
        if self.path == "/v4/linode/types/g6-nanode-1":
            self.send_json(200, {"id": "g6-nanode-1", "label": "Nanode 1 GB"})
            return
        if self.path == "/v4/networking/firewalls/123":
            self.send_json(200, {"id": 123, "label": "remote-e2e", "status": "enabled"})
            return
        firewall_prefix = "/v4/linode/instances/"
        if self.path.startswith(firewall_prefix) and "/firewalls?" in self.path:
            instance_part = self.path[len(firewall_prefix) :].split("/", 1)[0]
            try:
                instance_id = int(instance_part)
            except ValueError:
                self.send_json(404, {"errors": [{"reason": "not found"}]})
                return
            if self.server.state.get(instance_id) is None:
                self.send_json(404, {"errors": [{"reason": "not found"}]})
                return
            self.send_json(
                200,
                {
                    "data": [{"id": 123, "label": "remote-e2e", "status": "enabled"}],
                    "page": 1,
                    "pages": 1,
                    "results": 1,
                },
            )
            return
        if self.path.startswith("/v4/linode/instances?") or self.path == "/v4/linode/instances":
            label = None
            filter_value = self.headers.get("X-Filter")
            if filter_value:
                try:
                    decoded = json.loads(filter_value)
                    if isinstance(decoded, dict) and isinstance(decoded.get("label"), str):
                        label = decoded["label"]
                except json.JSONDecodeError:
                    self.send_json(400, {"errors": [{"reason": "invalid X-Filter"}]})
                    return
            data = self.server.state.list_by_label(label)
            self.send_json(200, {"data": data, "page": 1, "pages": 1, "results": len(data)})
            return
        prefix = "/v4/linode/instances/"
        if self.path.startswith(prefix):
            try:
                instance_id = int(self.path[len(prefix) :])
            except ValueError:
                self.send_json(404, {"errors": [{"reason": "not found"}]})
                return
            value = self.server.state.get(instance_id)
            if value is None:
                self.send_json(404, {"errors": [{"reason": "not found"}]})
            else:
                self.send_json(200, value)
            return
        self.send_json(404, {"errors": [{"reason": "not found"}]})

    def do_POST(self) -> None:  # noqa: N802
        r2_path = f"/client/v4/accounts/{R2_ACCOUNT_ID}/r2/temp-access-credentials"
        if self.path == r2_path:
            if self.headers.get("Authorization") != "Bearer remote-e2e-r2-api-token":
                self.send_json(
                    403,
                    {
                        "result": None,
                        "errors": [{"code": 10000, "message": "authentication failed"}],
                        "messages": [],
                        "success": False,
                    },
                )
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                request = json.loads(self.rfile.read(length))
            except (ValueError, json.JSONDecodeError):
                self.send_json(400, {"errors": [{"message": "invalid JSON"}], "success": False})
                return
            expected = {
                "bucket": R2_BUCKET,
                "parentAccessKeyId": R2_PARENT_ACCESS_KEY_ID,
                "permission": "object-read-write",
            }
            if any(request.get(key) != value for key, value in expected.items()):
                self.send_json(
                    400,
                    {
                        "result": None,
                        "errors": [{"code": 10001, "message": "invalid temporary credential scope"}],
                        "messages": [],
                        "success": False,
                    },
                )
                return
            prefixes = request.get("prefixes")
            ttl = request.get("ttlSeconds")
            minimum_ttl = 900 if prefixes == ["mcserver-preflight/"] else 3630
            if (
                not isinstance(prefixes, list)
                or len(prefixes) != 1
                or not isinstance(prefixes[0], str)
                or not prefixes[0].endswith("/")
                or not isinstance(ttl, int)
                or ttl < minimum_ttl
                or ttl > 604800
            ):
                self.send_json(
                    400,
                    {
                        "result": None,
                        "errors": [{"code": 10002, "message": "invalid TTL or prefix"}],
                        "messages": [],
                        "success": False,
                    },
                )
                return
            credentials = self.server.state.issue_r2_credentials(request)
            self.send_json(
                200,
                {
                    "result": credentials,
                    "errors": [],
                    "messages": [],
                    "success": True,
                },
            )
            return
        if self.path != "/v4/linode/instances":
            self.send_json(404, {"errors": [{"reason": "not found"}]})
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            request = json.loads(self.rfile.read(length))
        except (ValueError, json.JSONDecodeError):
            self.send_json(400, {"errors": [{"reason": "invalid JSON"}]})
            return
        if self.server.state.should_rate_limit_create():
            self.send_json(
                429,
                {"errors": [{"reason": "injected create rate limit"}]},
                {"Retry-After": "1"},
            )
            return
        required = {"region", "type", "image", "label", "authorized_keys", "metadata"}
        missing = sorted(required - request.keys())
        if missing:
            self.send_json(
                400,
                {"errors": [{"field": field, "reason": "required"} for field in missing]},
            )
            return
        value, lose_response = self.server.state.create(request)
        if lose_response:
            self.send_json(500, {"errors": [{"reason": "injected lost create response"}]})
        else:
            self.send_json(200, value)

    def do_DELETE(self) -> None:  # noqa: N802
        prefix = "/v4/linode/instances/"
        if not self.path.startswith(prefix):
            self.send_json(404, {"errors": [{"reason": "not found"}]})
            return
        try:
            instance_id = int(self.path[len(prefix) :])
        except ValueError:
            self.send_json(404, {"errors": [{"reason": "not found"}]})
            return
        existed, lose_response = self.server.state.delete(instance_id)
        if lose_response:
            self.send_json(500, {"errors": [{"reason": "injected lost delete response"}]})
        elif existed:
            self.send_json(200, {})
        else:
            self.send_json(404, {"errors": [{"reason": "not found"}]})

    def send_json(
        self,
        status: int,
        value: Any,
        headers: dict[str, str] | None = None,
    ) -> None:
        payload = json.dumps(value, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        for name, value in (headers or {}).items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, _format: str, *_arguments: object) -> None:
        return


class FakeAkamaiServer(ThreadingHTTPServer):
    def __init__(self, address: tuple[str, int], state: FakeAkamaiState) -> None:
        super().__init__(address, FakeAkamaiHandler)
        self.state = state


def generate_tls(work: Path, openssl: Path) -> tuple[Path, Path, Path, Path]:
    server_certificate = work / "remote-agent.crt"
    server_private_key = work / "remote-agent.key"
    client_ca_certificate = work / "agent-client-ca.crt"
    client_ca_private_key = work / "agent-client-ca.key"
    subprocess.run(
        [
            str(openssl),
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            str(server_private_key),
            "-out",
            str(server_certificate),
            "-days",
            "2",
            "-subj",
            "/CN=localhost",
            "-addext",
            "basicConstraints=critical,CA:FALSE",
            "-addext",
            "keyUsage=critical,digitalSignature,keyEncipherment",
            "-addext",
            "extendedKeyUsage=serverAuth",
            "-addext",
            "subjectAltName=DNS:localhost,IP:127.0.0.1",
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    subprocess.run(
        [
            str(openssl),
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            str(client_ca_private_key),
            "-out",
            str(client_ca_certificate),
            "-days",
            "2",
            "-subj",
            "/CN=mcserver remote E2E agent CA",
            "-addext",
            "basicConstraints=critical,CA:TRUE,pathlen:0",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return (
        server_certificate,
        server_private_key,
        client_ca_certificate,
        client_ca_private_key,
    )


def wait_for_socket(process: subprocess.Popen[bytes], path: Path, deadline: float, log: Path) -> None:
    while time.monotonic() < deadline:
        if process.poll() is not None:
            tail = log.read_text(errors="replace")[-12000:] if log.exists() else ""
            raise RuntimeError(
                f"control-plane exited before creating its socket with status "
                f"{process.returncode}\n{tail}"
            )
        if path.exists():
            return
        time.sleep(0.05)
    raise TimeoutError(f"timed out waiting for control-plane socket: {path}")


def bootstrap_script_from_request(request: dict[str, Any]) -> str:
    metadata = request.get("metadata")
    if not isinstance(metadata, dict) or not isinstance(metadata.get("user_data"), str):
        raise RuntimeError("Akamai create request has no metadata.user_data")
    return base64.b64decode(metadata["user_data"], validate=True).decode()


def bootstrap_environment_value(request: dict[str, Any], key: str) -> str:
    prefix = f"{key}="
    line = next(
        (
            line
            for line in bootstrap_script_from_request(request).splitlines()
            if line.startswith(prefix)
        ),
        None,
    )
    if line is None:
        raise RuntimeError(f"cloud-init user data has no {key} setting")
    return line[len(prefix) :]


def manifest_from_request(request: dict[str, Any]) -> dict[str, Any]:
    script = bootstrap_script_from_request(request)
    prefix = "# mcserver-bootstrap: "
    line = next((line for line in script.splitlines() if line.startswith(prefix)), None)
    if line is None:
        raise RuntimeError("cloud-init user data has no mcserver bootstrap manifest")
    return json.loads(base64.b64decode(line[len(prefix) :], validate=True))


def launch_new_agents(
    state: FakeAkamaiState,
    launched: dict[str, subprocess.Popen[bytes]],
    node_agent: Path,
    certificate: Path,
    remote_address: str,
    work: Path,
    fake_podman: Path,
    fake_restic: Path,
    runtime: Path,
) -> None:
    with state.lock:
        requests = list(state.requests)
    for request in requests:
        manifest = manifest_from_request(request)
        compute_id = str(manifest["compute_instance_id"])
        if compute_id in launched:
            continue
        if manifest["control_plane_address"] != remote_address:
            raise RuntimeError("bootstrap manifest contains unexpected control-plane address")
        max_frame_bytes = bootstrap_environment_value(
            request, "MCSERVER_NODE_AGENT_MAX_FRAME_BYTES"
        )
        command_timeout_seconds = bootstrap_environment_value(
            request, "MCSERVER_NODE_AGENT_COMMAND_TIMEOUT_SECONDS"
        )
        if max_frame_bytes != "131072":
            raise RuntimeError("cloud-init did not preserve the control-plane frame limit")
        if command_timeout_seconds != "25":
            raise RuntimeError("cloud-init did not leave the expected response timeout budget")
        environment = os.environ.copy()
        for key in list(environment):
            if key.startswith("RESTIC_") or key.startswith("AWS_"):
                environment.pop(key)
        environment.update(
            {
                "MCSERVER_NODE_AGENT_CONTROL_PLANE_ADDRESS": remote_address,
                "MCSERVER_NODE_AGENT_TLS_CA_CERTIFICATE": str(certificate),
                "MCSERVER_NODE_AGENT_TLS_SERVER_NAME": str(manifest["tls_server_name"]),
                "MCSERVER_NODE_AGENT_COMPUTE_INSTANCE_ID": compute_id,
                "MCSERVER_NODE_AGENT_CONNECTION_TOKEN": str(manifest["enrollment_token"]),
                "MCSERVER_NODE_AGENT_STATE_DIRECTORY": str(work / "remote-agents" / compute_id),
                "MCSERVER_NODE_AGENT_LOCAL_SCOPE": str(manifest["provider_scope"]),
                "MCSERVER_NODE_AGENT_DATA_ACCESS_MODE": "host",
                "MCSERVER_NODE_AGENT_MAX_FRAME_BYTES": max_frame_bytes,
                "MCSERVER_NODE_AGENT_COMMAND_TIMEOUT_SECONDS": command_timeout_seconds,
                "MCSERVER_NODE_AGENT_PODMAN_BINARY": str(fake_podman),
                "MCSERVER_NODE_AGENT_RESTIC_BINARY": str(fake_restic),
                "MCSERVER_NODE_AGENT_RESTIC_RETRY_LOCK_SECONDS": "1",
                "MCSERVER_NODE_AGENT_RECONNECT_MIN_SECONDS": "1",
                "MCSERVER_NODE_AGENT_RECONNECT_MAX_SECONDS": "2",
                "MCSERVER_FAKE_RUNTIME_DIRECTORY": str(runtime),
                "RUST_LOG": "mcserver_node_agent=info",
            }
        )
        log = (work / f"node-agent-{compute_id}.log").open("wb")
        launched[compute_id] = subprocess.Popen(
            [str(node_agent)],
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
        )


def terminate(process: subprocess.Popen[bytes]) -> None:
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
        "--openssl-binary",
        type=openssl_executable,
        default=os.environ.get("MCSERVER_CONTROL_PLANE_OPENSSL_BINARY", "openssl"),
        help=(
            "OpenSSL executable used by the test and control plane "
            "(default: MCSERVER_CONTROL_PLANE_OPENSSL_BINARY or openssl)"
        ),
    )
    parser.add_argument("--work-directory", type=Path, default=None)
    parser.add_argument("--keep-work-directory", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repository_root = Path(__file__).resolve().parent.parent
    local = load_local_e2e(repository_root)
    control_plane = require_executable(args.control_plane_binary, "control-plane binary")
    node_agent = require_executable(args.node_agent_binary, "node-agent binary")
    openssl = args.openssl_binary
    fake_podman = require_executable(repository_root / "scripts/fakes/podman.py", "fake Podman")
    fake_restic = require_executable(repository_root / "scripts/fakes/restic.py", "fake restic")

    temporary = args.work_directory is None
    work = Path(tempfile.mkdtemp(prefix="mcserver-remote-e2e-")) if temporary else args.work_directory.resolve()
    if not temporary:
        if work.exists():
            shutil.rmtree(work)
        work.mkdir(parents=True)

    socket_path = work / "control-plane.sock"
    database_path = work / "control-plane.db"
    control_log = work / "control-plane.log"
    runtime = work / "fake-runtime"
    repository = (
        f"s3:https://{R2_ACCOUNT_ID}.r2.cloudflarestorage.com/"
        f"{R2_BUCKET}/remote-e2e/repository"
    )
    (
        certificate,
        private_key,
        client_ca_certificate,
        client_ca_private_key,
    ) = generate_tls(work, openssl)
    authorized_keys = work / "authorized_keys"
    authorized_keys.write_text("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIE2Etest remote-e2e\n")
    agent_environment = work / "node-agent.env"
    agent_environment.write_text(
        "RESTIC_PASSWORD=remote-provider-e2e\n"
        "AWS_DEFAULT_REGION=auto\n"
    )
    api_state = FakeAkamaiState()
    orphan_provider_id = api_state.seed_orphan("remote-e2e")
    api_port = free_tcp_port()
    api_server = FakeAkamaiServer(("127.0.0.1", api_port), api_state)
    api_thread = threading.Thread(target=api_server.serve_forever, daemon=True)
    api_thread.start()

    local_agent_port = free_tcp_port()
    remote_agent_port = free_tcp_port()
    remote_address = f"127.0.0.1:{remote_agent_port}"
    environment = os.environ.copy()
    environment.update(
        {
            "MCSERVER_CONTROL_PLANE_SOCKET": str(socket_path),
            "MCSERVER_CONTROL_PLANE_DATABASE_URL": f"sqlite://{database_path}?mode=rwc",
            "MCSERVER_CONTROL_PLANE_AGENT_LISTEN_ADDRESS": f"127.0.0.1:{local_agent_port}",
            "MCSERVER_CONTROL_PLANE_NODE_AGENT_BINARY": str(node_agent),
            "MCSERVER_CONTROL_PLANE_NODE_AGENT_ROOT": str(work / "local-agents"),
            "MCSERVER_CONTROL_PLANE_PODMAN_BINARY": str(fake_podman),
            "MCSERVER_CONTROL_PLANE_REAP_ORPHANS_ON_START": "false",
            "MCSERVER_CONTROL_PLANE_RECONCILE_INTERVAL_SECONDS": "1",
            "MCSERVER_CONTROL_PLANE_RECONCILE_RETRY_SECONDS": "1",
            "MCSERVER_CONTROL_PLANE_AGENT_COMMAND_TIMEOUT_SECONDS": "30",
            "MCSERVER_CONTROL_PLANE_MAX_FRAME_BYTES": "131072",
            "MCSERVER_CONTROL_PLANE_SHUTDOWN_TIMEOUT_SECONDS": "5",
            "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_LISTEN_ADDRESS": remote_address,
            "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_PUBLIC_ADDRESS": remote_address,
            "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_SERVER_NAME": "localhost",
            "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CERTIFICATE": str(certificate),
            "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_PRIVATE_KEY": str(private_key),
            "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CA_CERTIFICATE": str(certificate),
            "MCSERVER_CONTROL_PLANE_AGENT_CLIENT_CA_CERTIFICATE": str(client_ca_certificate),
            "MCSERVER_CONTROL_PLANE_AGENT_CLIENT_CA_PRIVATE_KEY": str(client_ca_private_key),
            "MCSERVER_CONTROL_PLANE_AGENT_CERTIFICATE_WORK_DIRECTORY": str(work / "agent-pki"),
            "MCSERVER_CONTROL_PLANE_AGENT_CERTIFICATE_VALIDITY_SECONDS": "7200",
            "MCSERVER_CONTROL_PLANE_OPENSSL_BINARY": str(openssl),
            "MCSERVER_CONTROL_PLANE_AGENT_TRUST_DOMAIN": "remote-e2e.invalid",
            "MCSERVER_CONTROL_PLANE_NODE_AGENT_DOWNLOAD_URL": "https://example.invalid/mcserver-node-agent",
            "MCSERVER_CONTROL_PLANE_NODE_AGENT_SHA256": "0" * 64,
            "MCSERVER_AKAMAI_API_TOKEN": "remote-e2e-token",
            "MCSERVER_AKAMAI_API_BASE_URL": f"http://127.0.0.1:{api_port}/v4",
            "MCSERVER_AKAMAI_AUTHORIZED_KEYS_FILE": str(authorized_keys),
            "MCSERVER_AKAMAI_SCOPE": "remote-e2e",
            "MCSERVER_AKAMAI_REQUEST_TIMEOUT_SECONDS": "5",
            "MCSERVER_AKAMAI_REAP_ORPHANS_ON_START": "true",
            "MCSERVER_AKAMAI_LIVE_ENABLED": "true",
            "MCSERVER_AKAMAI_REGION": AKAMAI_REGION,
            "MCSERVER_AKAMAI_IMAGE": AKAMAI_IMAGE,
            "MCSERVER_AKAMAI_FIREWALL_ID": "123",
            "MCSERVER_AKAMAI_ALLOWED_INSTANCE_TYPES": "g6-nanode-1",
            "MCSERVER_AKAMAI_MAX_ACTIVE_INSTANCES": "1",
            "MCSERVER_AKAMAI_MAX_INSTANCE_LIFETIME_SECONDS": "3600",
            "MCSERVER_R2_API_TOKEN": "remote-e2e-r2-api-token",
            "MCSERVER_R2_API_BASE_URL": f"http://127.0.0.1:{api_port}/client/v4",
            "MCSERVER_R2_ACCOUNT_ID": R2_ACCOUNT_ID,
            "MCSERVER_R2_PARENT_ACCESS_KEY_ID": R2_PARENT_ACCESS_KEY_ID,
            "MCSERVER_R2_BUCKET": R2_BUCKET,
            "MCSERVER_R2_TEMPORARY_CREDENTIAL_TTL_SECONDS": "7200",
            "MCSERVER_R2_RUNTIME_ENVIRONMENT_FILE": str(agent_environment),
            "MCSERVER_R2_REQUEST_TIMEOUT_SECONDS": "5",
            "MCSERVER_FAKE_RUNTIME_DIRECTORY": str(runtime),
            "RUST_LOG": "mcserver_control_plane=info,mcserver_node_agent=info",
        }
    )

    process: subprocess.Popen[bytes] | None = None
    launched: dict[str, subprocess.Popen[bytes]] = {}
    succeeded = False
    try:
        init_environment = environment.copy()
        init_environment.update(
            {
                "RESTIC_PASSWORD": "remote-provider-e2e",
                "AWS_ACCESS_KEY_ID": "initialization-access",
                "AWS_SECRET_ACCESS_KEY": "initialization-secret",
                "AWS_SESSION_TOKEN": "initialization-session",
                "AWS_DEFAULT_REGION": "auto",
            }
        )
        subprocess.run(
            [str(fake_restic), "--repo", str(repository), "init"],
            env=init_environment,
            check=True,
        )
        with control_log.open("wb") as log:
            process = subprocess.Popen(
                [str(control_plane)],
                cwd=repository_root,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            wait_for_socket(process, socket_path, time.monotonic() + 15, control_log)
            client = local.JsonRpcClient(socket_path, timeout_seconds=5)
            ping = client.call("system.ping")
            if ping.get("status") != "ok":
                raise RuntimeError(f"unexpected ping result: {ping!r}")
            server = client.call(
                "server.create",
                {
                    "name": f"remote-e2e-{uuid.uuid4()}",
                    "spec": {
                        "compute": {
                            "provider": "akamai",
                            "region": AKAMAI_REGION,
                            "instance_type": "g6-nanode-1",
                            "image": AKAMAI_IMAGE,
                            "firewall_id": 123,
                        },
                        "process": {
                            "container_image": "fake/minecraft-server:latest",
                            "server_type": "VANILLA",
                            "version": "LATEST",
                            "host_port": 25565,
                            "stop_timeout_seconds": 1,
                            "accept_eula": True,
                            "environment": {},
                        },
                        "data": {"repository": repository},
                    },
                },
            )
            server_id = str(server["id"])
            client.call(
                "server.set_desired_state",
                {
                    "server_id": server_id,
                    "desired_state": "running",
                    "expected_generation": server["generation"],
                },
            )
            previous: str | None = None
            snapshots: list[str] = []
            for generation in (1, 2):
                deadline = time.monotonic() + 60
                while time.monotonic() < deadline:
                    launch_new_agents(
                        api_state,
                        launched,
                        node_agent,
                        certificate,
                        remote_address,
                        work,
                        fake_podman,
                        fake_restic,
                        runtime,
                    )
                    instance = local.active_instance(client, server_id)
                    if (
                        instance is not None
                        and instance["id"] != previous
                        and instance["process_running"]
                        and instance["data_prepared_at_ms"] is not None
                    ):
                        break
                    time.sleep(0.1)
                else:
                    raise TimeoutError(f"generation {generation} did not become running")
                if int(instance["fencing_token"]) != generation:
                    raise RuntimeError(f"unexpected fencing token: {instance!r}")
                expected_source = None if generation == 1 else snapshots[-1]
                if instance.get("source_snapshot_id") != expected_source:
                    raise RuntimeError(f"unexpected source snapshot: {instance!r}")
                current = client.call("server.get", {"server_id": server_id})
                client.call(
                    "server.set_desired_state",
                    {
                        "server_id": server_id,
                        "desired_state": "stopped",
                        "expected_generation": current["generation"],
                    },
                )
                completed = local.wait_for_completed_instance(
                    client, server_id, str(instance["id"]), time.monotonic() + 60
                )
                snapshots.append(str(completed["result_snapshot_id"]))
                previous = str(instance["id"])
                if generation == 1:
                    current = client.call("server.get", {"server_id": server_id})
                    client.call(
                        "server.set_desired_state",
                        {
                            "server_id": server_id,
                            "desired_state": "running",
                            "expected_generation": current["generation"],
                        },
                    )

            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if not api_state.list_by_label(None) and all(
                    child.poll() is not None for child in launched.values()
                ):
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError("remote provider cleanup did not delete VMs and agents")

            with api_state.lock:
                if len(api_state.requests) != 2 or len(api_state.deleted) != 3:
                    raise RuntimeError(
                        f"unexpected provider lifecycle: creates={len(api_state.requests)} "
                        f"deletes={len(api_state.deleted)}"
                    )
                if orphan_provider_id not in api_state.deleted:
                    raise RuntimeError("startup reaper did not delete the seeded orphan VM")
                if not api_state.rate_limit_injected:
                    raise RuntimeError("create rate limit was not injected")
                if not api_state.lost_create_response_injected:
                    raise RuntimeError("lost create response was not injected")
                if not api_state.lost_delete_response_injected:
                    raise RuntimeError("lost delete response was not injected")
                if api_state.image_preflight_checks < 2:
                    raise RuntimeError(
                        "image capability preflight did not run for both generations: "
                        f"{api_state.image_preflight_checks}"
                    )
                if api_state.region_preflight_checks < 2:
                    raise RuntimeError(
                        "region capability preflight did not run for both generations: "
                        f"{api_state.region_preflight_checks}"
                    )
                if len(api_state.r2_credential_requests) < 3:
                    raise RuntimeError(
                        "R2 temporary credentials were not issued for preflight and both "
                        f"remote registrations: {api_state.r2_credential_requests!r}"
                    )
                prefixes = [
                    request["prefixes"][0]
                    for request in api_state.r2_credential_requests
                ]
                if prefixes.count("mcserver-preflight/") != 1:
                    raise RuntimeError(f"unexpected R2 preflight scopes: {prefixes!r}")
                if prefixes.count("remote-e2e/repository/") < 2:
                    raise RuntimeError(f"remote R2 credentials were not prefix-scoped: {prefixes!r}")
                for request in api_state.requests:
                    if request.get("region") != AKAMAI_REGION:
                        raise RuntimeError("Akamai create request used the wrong region")
                    if request.get("image") != AKAMAI_IMAGE:
                        raise RuntimeError("Akamai create request used the wrong image")
                    script = bootstrap_script_from_request(request)
                    if "remote-provider-e2e" in script or "RESTIC_PASSWORD" in script:
                        raise RuntimeError("runtime storage secrets leaked into cloud-init user data")
                    manifest = manifest_from_request(request)
                    state_directory = (
                        work / "remote-agents" / str(manifest["compute_instance_id"])
                    )
                    credential_path = state_directory / "connection-token"
                    certificate_path = state_directory / "client-certificate-chain.pem"
                    private_key_path = state_directory / "client-private-key.pem"
                    if not credential_path.is_file():
                        raise RuntimeError("remote agent did not persist its reconnect token")
                    if not certificate_path.is_file() or not private_key_path.is_file():
                        raise RuntimeError("remote agent did not persist its mTLS identity")
                    reconnect_token = credential_path.read_text().strip()
                    if (
                        reconnect_token == manifest["enrollment_token"]
                        or len(reconnect_token) != 64
                    ):
                        raise RuntimeError("remote agent reconnect token was not rotated")
                    subprocess.run(
                        [
                            str(openssl),
                            "verify",
                            "-purpose",
                            "sslclient",
                            "-CAfile",
                            str(client_ca_certificate),
                            str(certificate_path),
                        ],
                        check=True,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                    )
                    if request.get("booted") is not True:
                        raise RuntimeError("Akamai create request did not request boot")
                    if request.get("firewall_id") != 123:
                        raise RuntimeError("Akamai create request lost firewall id")
                    if "mcserver-managed" not in request.get("tags", []):
                        raise RuntimeError("Akamai create request has no ownership tag")
            with sqlite3.connect(database_path) as database:
                enrollment_state = database.execute(
                    """
                    SELECT
                        count(*) FILTER (WHERE enrollment_token IS NOT NULL),
                        count(*) FILTER (
                            WHERE agent_csr_pem IS NOT NULL
                              AND agent_certificate_chain_pem IS NOT NULL
                              AND agent_certificate_der IS NOT NULL
                              AND agent_certificate_expires_at_ms > created_at_ms
                        )
                    FROM compute_instances
                    WHERE provider = 'akamai'
                    """
                ).fetchone()
            if enrollment_state[0] != 0:
                raise RuntimeError("remote enrollment token was not invalidated")
            if enrollment_state[1] != 2:
                raise RuntimeError(
                    f"remote mTLS certificates were not persisted: {enrollment_state!r}"
                )

            succeeded = True
            print(
                "remote provider E2E passed: mTLS certificate enrollment, prefix-scoped R2 "
                "temporary credentials, startup orphan reaping, rate-limit handling, uncertain "
                "create/delete recovery, two generations, snapshot restore, and VM deletion "
                "succeeded"
            )
    except Exception as error:  # noqa: BLE001
        print(f"remote provider E2E failed: {error}", file=sys.stderr)
        if control_log.exists():
            print("--- control-plane.log (tail) ---", file=sys.stderr)
            print("\n".join(control_log.read_text(errors="replace").splitlines()[-160:]), file=sys.stderr)
        return 1
    finally:
        for child in launched.values():
            terminate(child)
        if process is not None:
            terminate(process)
        api_server.shutdown()
        api_server.server_close()
        api_thread.join(timeout=5)
        if temporary and succeeded and not args.keep_work_directory:
            shutil.rmtree(work, ignore_errors=True)
        else:
            print(f"artifacts retained at {work}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
