#!/usr/bin/env python3
"""Install and verify one production mcserver control plane.

The script runs on the AlmaLinux control-plane host. It deliberately does not
create DNS records, Cloudflare credentials, Akamai credentials, or firewall
rules. Those account-level resources are inputs. Everything from immutable
release verification through the no-create preflight and optional live
acceptance test is automated and recorded in a secret-free JSON report.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

CONFIRMATION = "I_UNDERSTAND_THIS_CREATES_BILLABLE_AKAMAI_RESOURCES"
REPORT_SCHEMA = 1
HEX_64 = re.compile(r"^[0-9a-f]{64}$")
HEX_40 = re.compile(r"^[0-9a-f]{40}$")
ACCOUNT_ID = re.compile(r"^[0-9a-fA-F]{32}$")
RELEASE_VERSION = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+$")
SAFE_REPOSITORY = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
PROVIDER_IDENTIFIER = re.compile(r"^[A-Za-z0-9._/-]+$")
PEM_CERTIFICATE = re.compile(
    rb"-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----",
    re.DOTALL,
)
MAX_PKI_INPUT_BYTES = 1024 * 1024


class DeployError(RuntimeError):
    """A production deployment validation or execution failure."""


@dataclass
class Report:
    command: str
    config: str
    release: str = ""
    started_at: str = field(
        default_factory=lambda: dt.datetime.now(dt.timezone.utc).isoformat()
    )
    finished_at: str | None = None
    outcome: str = "running"
    error: str | None = None
    steps: list[dict[str, str]] = field(default_factory=list)

    def record(self, name: str, detail: str = "ok") -> None:
        self.steps.append({"name": name, "status": "passed", "detail": detail})
        print(f"[passed] {name}: {detail}", flush=True)

    def fail(self, error: BaseException) -> None:
        self.outcome = "failed"
        self.error = str(error)
        self.finished_at = dt.datetime.now(dt.timezone.utc).isoformat()

    def succeed(self) -> None:
        self.outcome = "passed"
        self.finished_at = dt.datetime.now(dt.timezone.utc).isoformat()

    def write(self, path: Path) -> None:
        payload = {
            "schema": REPORT_SCHEMA,
            "command": self.command,
            "config": self.config,
            "release": self.release,
            "started_at": self.started_at,
            "finished_at": self.finished_at,
            "outcome": self.outcome,
            "error": self.error,
            "steps": self.steps,
        }
        path = path.expanduser().resolve()
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
        temporary.write_text(
            json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        temporary.chmod(0o640)
        os.replace(temporary, path)
        print(f"report={path}", flush=True)


@dataclass(frozen=True)
class Release:
    version: str
    repository: str
    target: str
    checksums_sha256: str
    expected_commit: str

    @property
    def version_number(self) -> str:
        return self.version.removeprefix("v")

    @property
    def archive_name(self) -> str:
        return (
            f"minecraft-server-management-{self.version}-{self.target}.tar.gz"
        )

    def asset_name(self, binary: str) -> str:
        return f"{binary}-{self.version}-{self.target}"

    def asset_url(self, name: str) -> str:
        return (
            f"https://github.com/{self.repository}/releases/download/"
            f"{self.version}/{name}"
        )


@dataclass(frozen=True)
class Config:
    source_path: Path
    release: Release
    service: dict[str, Any]
    akamai: dict[str, Any]
    r2: dict[str, Any]
    files: dict[str, Path]
    acceptance: dict[str, Any]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Check, deploy, and verify a production mcserver control plane."
    )
    parser.add_argument(
        "command",
        choices=("check", "deploy", "verify"),
        help=(
            "check validates inputs and release artifacts; deploy installs and "
            "runs the no-create preflight; verify checks an existing deployment"
        ),
    )
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument(
        "--report",
        type=Path,
        default=Path("/var/lib/mcserver-deploy/production-report.json"),
        help="secret-free JSON result report",
    )
    parser.add_argument(
        "--go-live",
        action="store_true",
        help="after preflight, enable live creation and run two-generation acceptance",
    )
    parser.add_argument(
        "--confirm-billable-akamai-run",
        metavar="PHRASE",
        help=f"with --go-live, must equal {CONFIRMATION}",
    )
    parser.add_argument(
        "--accept-minecraft-eula",
        action="store_true",
        help="required with --go-live",
    )
    return parser.parse_args()


def require_table(document: dict[str, Any], name: str) -> dict[str, Any]:
    value = document.get(name)
    if not isinstance(value, dict):
        raise DeployError(f"missing TOML table [{name}]")
    return value


def reject_unknown_keys(
    table: dict[str, Any], allowed: set[str], table_name: str
) -> None:
    unknown = set(table) - allowed
    if unknown:
        names = ", ".join(sorted(unknown))
        raise DeployError(f"unknown {table_name} key(s): {names}")


def require_string(table: dict[str, Any], key: str, table_name: str) -> str:
    value = table.get(key)
    if not isinstance(value, str) or not value.strip():
        raise DeployError(f"{table_name}.{key} must be a non-empty string")
    if "\0" in value or "\n" in value or "\r" in value:
        raise DeployError(f"{table_name}.{key} contains a forbidden character")
    if "REPLACE_" in value:
        raise DeployError(f"{table_name}.{key} still contains a placeholder")
    return value


def require_positive_int(table: dict[str, Any], key: str, table_name: str) -> int:
    value = table.get(key)
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise DeployError(f"{table_name}.{key} must be a positive integer")
    return value


def require_string_list(
    table: dict[str, Any], key: str, table_name: str
) -> list[str]:
    value = table.get(key)
    if not isinstance(value, list) or not value:
        raise DeployError(f"{table_name}.{key} must be a non-empty string array")
    result: list[str] = []
    for item in value:
        if (
            not isinstance(item, str)
            or item != item.strip()
            or not PROVIDER_IDENTIFIER.fullmatch(item)
        ):
            raise DeployError(f"{table_name}.{key} contains an invalid value")
        result.append(item)
    if len(set(result)) != len(result):
        raise DeployError(f"{table_name}.{key} contains a duplicate value")
    return result


def require_positive_int_list(
    table: dict[str, Any], key: str, table_name: str
) -> list[int]:
    value = table.get(key)
    if not isinstance(value, list) or not value:
        raise DeployError(f"{table_name}.{key} must be a non-empty integer array")
    if any(
        not isinstance(item, int) or isinstance(item, bool) or item <= 0
        for item in value
    ):
        raise DeployError(f"{table_name}.{key} contains an invalid value")
    if len(set(value)) != len(value):
        raise DeployError(f"{table_name}.{key} contains a duplicate value")
    return value


def require_positive_number(
    table: dict[str, Any], key: str, table_name: str
) -> float:
    value = table.get(key)
    if (
        not isinstance(value, (int, float))
        or isinstance(value, bool)
        or value <= 0
    ):
        raise DeployError(f"{table_name}.{key} must be a positive number")
    return float(value)


def resolve_input_path(config_path: Path, raw: str) -> Path:
    path = Path(raw).expanduser()
    if not path.is_absolute():
        path = config_path.parent / path
    return path.resolve()


def load_config(path: Path) -> Config:
    path = path.expanduser().resolve()
    try:
        with path.open("rb") as source:
            document = tomllib.load(source)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise DeployError(f"cannot read deployment config {path}: {error}") from error

    release_table = require_table(document, "release")
    service = require_table(document, "service")
    akamai = require_table(document, "akamai")
    r2 = require_table(document, "r2")
    files_table = require_table(document, "files")
    acceptance = require_table(document, "acceptance")
    reject_unknown_keys(
        document,
        {"release", "service", "akamai", "r2", "files", "acceptance"},
        "top-level",
    )
    reject_unknown_keys(
        release_table,
        {
            "version",
            "repository",
            "target",
            "checksums_sha256",
            "expected_commit",
        },
        "release",
    )
    reject_unknown_keys(
        service,
        {
            "public_address",
            "server_name",
            "trust_domain",
            "rust_log",
            "certbot_lineage",
        },
        "service",
    )
    reject_unknown_keys(
        akamai,
        {
            "scope",
            "allowed_regions",
            "allowed_images",
            "allowed_instance_types",
            "allowed_firewall_ids",
            "max_active_instances",
            "max_instance_lifetime_seconds",
        },
        "akamai",
    )
    reject_unknown_keys(
        r2,
        {
            "account_id",
            "parent_access_key_id",
            "bucket",
            "temporary_credential_ttl_seconds",
        },
        "r2",
    )
    reject_unknown_keys(
        files_table,
        {
            "akamai_api_token",
            "r2_api_token",
            "remote_tls_private_key",
            "agent_client_ca_private_key",
            "r2_runtime_environment",
            "remote_tls_fullchain",
            "remote_tls_root_ca",
            "agent_client_ca_certificate",
            "authorized_keys",
        },
        "files",
    )
    reject_unknown_keys(
        acceptance,
        {
            "region",
            "image",
            "instance_type",
            "firewall_id",
            "host_port",
            "timeout_seconds",
        },
        "acceptance",
    )

    release = Release(
        version=require_string(release_table, "version", "release"),
        repository=require_string(release_table, "repository", "release"),
        target=require_string(release_table, "target", "release"),
        checksums_sha256=require_string(
            release_table, "checksums_sha256", "release"
        ).lower(),
        expected_commit=require_string(
            release_table, "expected_commit", "release"
        ).lower(),
    )
    required_files = (
        "akamai_api_token",
        "r2_api_token",
        "remote_tls_private_key",
        "agent_client_ca_private_key",
        "r2_runtime_environment",
        "remote_tls_fullchain",
        "remote_tls_root_ca",
        "agent_client_ca_certificate",
        "authorized_keys",
    )
    files = {
        key: resolve_input_path(
            path, require_string(files_table, key, "files")
        )
        for key in required_files
    }
    return Config(
        source_path=path,
        release=release,
        service=service,
        akamai=akamai,
        r2=r2,
        files=files,
        acceptance=acceptance,
    )


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def validate_secret(path: Path, name: str) -> None:
    if not path.is_file():
        raise DeployError(f"files.{name} is not a regular file: {path}")
    mode = stat.S_IMODE(path.stat().st_mode)
    if mode & 0o077:
        raise DeployError(
            f"files.{name} must not be accessible by group or other "
            f"(current mode {mode:04o}): {path}"
        )
    if path.stat().st_size == 0:
        raise DeployError(f"files.{name} is empty: {path}")


def parse_environment_file(path: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    for line_number, raw in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        key, separator, value = line.partition("=")
        if not separator or not key or not value:
            raise DeployError(f"invalid environment line {path}:{line_number}")
        if key in result:
            raise DeployError(f"duplicate environment key {key} in {path}")
        result[key] = value
    return result


def validate_config(config: Config) -> None:
    release = config.release
    if not RELEASE_VERSION.fullmatch(release.version):
        raise DeployError("release.version must have the form vMAJOR.MINOR.PATCH")
    if not SAFE_REPOSITORY.fullmatch(release.repository):
        raise DeployError("release.repository must have the form OWNER/REPOSITORY")
    if not HEX_64.fullmatch(release.checksums_sha256):
        raise DeployError("release.checksums_sha256 must be 64 lowercase hex characters")
    if not HEX_40.fullmatch(release.expected_commit):
        raise DeployError("release.expected_commit must be a full Git commit SHA")
    if release.target != "x86_64-unknown-linux-musl":
        raise DeployError("only the released x86_64-unknown-linux-musl target is supported")

    public_address = require_string(
        config.service, "public_address", "service"
    )
    server_name = require_string(config.service, "server_name", "service")
    if public_address != f"{server_name}:443":
        raise DeployError("service.public_address must exactly equal service.server_name:443")
    require_string(config.service, "trust_domain", "service")
    certbot_lineage = Path(
        require_string(config.service, "certbot_lineage", "service")
    )
    if not certbot_lineage.is_absolute():
        raise DeployError("service.certbot_lineage must be an absolute path")
    if "rust_log" in config.service:
        require_string(config.service, "rust_log", "service")

    require_string(config.akamai, "scope", "akamai")
    allowed_regions = require_string_list(
        config.akamai, "allowed_regions", "akamai"
    )
    allowed_images = require_string_list(
        config.akamai, "allowed_images", "akamai"
    )
    allowed_instance_types = require_string_list(
        config.akamai, "allowed_instance_types", "akamai"
    )
    allowed_firewall_ids = require_positive_int_list(
        config.akamai, "allowed_firewall_ids", "akamai"
    )
    if require_positive_int(config.akamai, "max_active_instances", "akamai") != 1:
        raise DeployError("akamai.max_active_instances must be 1 for initial production")
    maximum_lifetime = require_positive_int(
        config.akamai, "max_instance_lifetime_seconds", "akamai"
    )

    account_id = require_string(config.r2, "account_id", "r2")
    if not ACCOUNT_ID.fullmatch(account_id):
        raise DeployError("r2.account_id must be 32 hexadecimal characters")
    require_string(config.r2, "parent_access_key_id", "r2")
    bucket = require_string(config.r2, "bucket", "r2")
    if "/" in bucket:
        raise DeployError("r2.bucket must be a bucket name without a path")
    ttl = require_positive_int(
        config.r2, "temporary_credential_ttl_seconds", "r2"
    )
    if ttl < maximum_lifetime + 3600 or ttl > 604800:
        raise DeployError(
            "r2 temporary credential TTL must cover maximum VM lifetime plus "
            "one hour and must not exceed seven days"
        )

    secret_names = (
        "akamai_api_token",
        "r2_api_token",
        "remote_tls_private_key",
        "agent_client_ca_private_key",
        "r2_runtime_environment",
    )
    for name in secret_names:
        validate_secret(config.files[name], name)
    for name in (
        "remote_tls_fullchain",
        "remote_tls_root_ca",
        "agent_client_ca_certificate",
        "authorized_keys",
    ):
        path = config.files[name]
        if not path.is_file() or path.stat().st_size == 0:
            raise DeployError(f"files.{name} is missing or empty: {path}")

    runtime = parse_environment_file(config.files["r2_runtime_environment"])
    if set(runtime) != {"AWS_DEFAULT_REGION"}:
        raise DeployError(
            "R2 runtime environment must contain exactly AWS_DEFAULT_REGION"
        )
    if runtime["AWS_DEFAULT_REGION"] != "auto":
        raise DeployError("AWS_DEFAULT_REGION must be auto for Cloudflare R2")
    acceptance_region = require_string(
        config.acceptance, "region", "acceptance"
    )
    acceptance_image = require_string(config.acceptance, "image", "acceptance")
    acceptance_type = require_string(
        config.acceptance, "instance_type", "acceptance"
    )
    acceptance_firewall = require_positive_int(
        config.acceptance, "firewall_id", "acceptance"
    )
    if acceptance_region not in allowed_regions:
        raise DeployError("acceptance.region is not in akamai.allowed_regions")
    if acceptance_image not in allowed_images:
        raise DeployError("acceptance.image is not in akamai.allowed_images")
    if acceptance_type not in allowed_instance_types:
        raise DeployError(
            "acceptance.instance_type is not in akamai.allowed_instance_types"
        )
    if acceptance_firewall not in allowed_firewall_ids:
        raise DeployError(
            "acceptance.firewall_id is not in akamai.allowed_firewall_ids"
        )
    host_port = require_positive_int(config.acceptance, "host_port", "acceptance")
    if host_port > 65535:
        raise DeployError("acceptance.host_port must not exceed 65535")
    require_positive_number(config.acceptance, "timeout_seconds", "acceptance")


def download(url: str, destination: Path) -> None:
    request = urllib.request.Request(
        url, headers={"User-Agent": "mcserver-production-deploy/1"}
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            if response.status != 200:
                raise DeployError(f"download returned HTTP {response.status}: {url}")
            with destination.open("wb") as output:
                shutil.copyfileobj(response, output)
    except (OSError, urllib.error.URLError) as error:
        raise DeployError(f"download failed for {url}: {error}") from error


def parse_checksums(path: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        digest, separator, name = line.partition("  ")
        if not separator or not HEX_64.fullmatch(digest):
            raise DeployError(f"invalid SHA256SUMS line: {line!r}")
        if "/" in name or name in result:
            raise DeployError(f"invalid or duplicate SHA256SUMS asset: {name!r}")
        result[name] = digest
    return result


def safe_extract(archive: Path, destination: Path) -> None:
    with tarfile.open(archive, "r:gz") as package:
        for member in package.getmembers():
            member_path = Path(member.name)
            if member_path.is_absolute() or ".." in member_path.parts:
                raise DeployError(f"unsafe release archive member: {member.name}")
            if member.issym() or member.islnk() or member.isdev():
                raise DeployError(f"unsupported release archive member: {member.name}")
        package.extractall(destination, filter="data")


def verify_release(config: Config, work: Path, report: Report) -> Path:
    release = config.release
    checksums_path = work / "SHA256SUMS"
    download(release.asset_url("SHA256SUMS"), checksums_path)
    actual_manifest_digest = sha256(checksums_path)
    if actual_manifest_digest != release.checksums_sha256:
        raise DeployError(
            "SHA256SUMS does not match the digest pinned in deployment config"
        )
    report.record("release checksum manifest", actual_manifest_digest)

    checksums = parse_checksums(checksums_path)
    expected_archive_digest = checksums.get(release.archive_name)
    if expected_archive_digest is None:
        raise DeployError(f"release manifest omits {release.archive_name}")
    archive = work / release.archive_name
    download(release.asset_url(release.archive_name), archive)
    if sha256(archive) != expected_archive_digest:
        raise DeployError("release archive checksum mismatch")
    report.record("release archive", expected_archive_digest)

    package = work / "package"
    package.mkdir()
    safe_extract(archive, package)
    metadata = parse_environment_file(package / "BUILD-METADATA")
    if metadata.get("version") != release.version_number:
        raise DeployError("release BUILD-METADATA version mismatch")
    if metadata.get("git_commit") != release.expected_commit:
        raise DeployError("release BUILD-METADATA commit mismatch")
    if metadata.get("target") != release.target:
        raise DeployError("release BUILD-METADATA target mismatch")

    for binary in ("mcserver-control-plane", "mcserver-node-agent", "mcserverctl"):
        binary_path = package / binary
        if not binary_path.is_file():
            raise DeployError(f"release archive omits {binary}")
        asset_digest = checksums.get(release.asset_name(binary))
        if asset_digest is not None and sha256(binary_path) != asset_digest:
            raise DeployError(f"{binary} checksum mismatch")
    node_digest = sha256(package / "mcserver-node-agent")
    report.record("release metadata", release.expected_commit)
    report.record("node-agent digest", node_digest)
    return package


def run(
    command: list[str],
    *,
    timeout: float = 120,
    capture: bool = True,
) -> subprocess.CompletedProcess[str]:
    printable = " ".join(command)
    print(f"+ {printable}", flush=True)
    try:
        return subprocess.run(
            command,
            check=True,
            text=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE if capture else None,
            stderr=subprocess.STDOUT if capture else None,
            timeout=timeout,
        )
    except subprocess.CalledProcessError as error:
        output = (error.stdout or "").strip()
        if len(output) > 8000:
            output = output[-8000:]
        raise DeployError(
            f"command failed ({error.returncode}): {printable}\n{output}"
        ) from error
    except (OSError, subprocess.TimeoutExpired) as error:
        raise DeployError(f"cannot run command: {printable}: {error}") from error


def render_environment(config: Config, node_digest: str, live: bool) -> str:
    release = config.release
    service = config.service
    akamai = config.akamai
    r2 = config.r2
    values = {
        "RUST_LOG": service.get(
            "rust_log", "mcserver_control_plane=info,mcserver_protocol=info"
        ),
        "MCSERVER_CONTROL_PLANE_SOCKET": "/run/mcserver/control-plane.sock",
        "MCSERVER_CONTROL_PLANE_DATABASE_URL": (
            "sqlite:///var/lib/mcserver/control-plane.db?mode=rwc"
        ),
        "MCSERVER_CONTROL_PLANE_SOCKET_MODE": "0660",
        "MCSERVER_CONTROL_PLANE_AGENT_LISTEN_ADDRESS": "127.0.0.1:39001",
        "MCSERVER_CONTROL_PLANE_REAP_ORPHANS_ON_START": "false",
        "MCSERVER_CONTROL_PLANE_RECONCILE_INTERVAL_SECONDS": "30",
        "MCSERVER_CONTROL_PLANE_RECONCILE_RETRY_SECONDS": "5",
        "MCSERVER_CONTROL_PLANE_AGENT_COMMAND_TIMEOUT_SECONDS": "900",
        "MCSERVER_CONTROL_PLANE_SHUTDOWN_TIMEOUT_SECONDS": "20",
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_LISTEN_ADDRESS": "0.0.0.0:443",
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_PUBLIC_ADDRESS": service[
            "public_address"
        ],
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_SERVER_NAME": service[
            "server_name"
        ],
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CERTIFICATE": (
            "/etc/mcserver/pki/remote-tls-fullchain.pem"
        ),
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CA_CERTIFICATE": (
            "/etc/mcserver/pki/remote-tls-root-ca.pem"
        ),
        "MCSERVER_CONTROL_PLANE_AGENT_CLIENT_CA_CERTIFICATE": (
            "/etc/mcserver/pki/agent-client-ca.pem"
        ),
        "MCSERVER_CONTROL_PLANE_AGENT_CERTIFICATE_WORK_DIRECTORY": (
            "/run/mcserver/agent-pki"
        ),
        "MCSERVER_CONTROL_PLANE_AGENT_CERTIFICATE_VALIDITY_SECONDS": "172800",
        "MCSERVER_CONTROL_PLANE_AGENT_TRUST_DOMAIN": service["trust_domain"],
        "MCSERVER_CONTROL_PLANE_OPENSSL_BINARY": "/usr/bin/openssl",
        "MCSERVER_CONTROL_PLANE_NODE_AGENT_DOWNLOAD_URL": release.asset_url(
            release.asset_name("mcserver-node-agent")
        ),
        "MCSERVER_CONTROL_PLANE_NODE_AGENT_SHA256": node_digest,
        "MCSERVER_AKAMAI_API_BASE_URL": "https://api.linode.com/v4",
        "MCSERVER_AKAMAI_AUTHORIZED_KEYS_FILE": "/etc/mcserver/authorized_keys",
        "MCSERVER_AKAMAI_SCOPE": akamai["scope"],
        "MCSERVER_AKAMAI_REQUEST_TIMEOUT_SECONDS": "30",
        "MCSERVER_AKAMAI_REAP_ORPHANS_ON_START": "false",
        "MCSERVER_AKAMAI_LIVE_ENABLED": str(live).lower(),
        "MCSERVER_AKAMAI_ALLOWED_REGIONS": ",".join(akamai["allowed_regions"]),
        "MCSERVER_AKAMAI_ALLOWED_IMAGES": ",".join(akamai["allowed_images"]),
        "MCSERVER_AKAMAI_ALLOWED_INSTANCE_TYPES": ",".join(
            akamai["allowed_instance_types"]
        ),
        "MCSERVER_AKAMAI_ALLOWED_FIREWALL_IDS": ",".join(
            str(value) for value in akamai["allowed_firewall_ids"]
        ),
        "MCSERVER_AKAMAI_MAX_ACTIVE_INSTANCES": str(
            akamai["max_active_instances"]
        ),
        "MCSERVER_AKAMAI_MAX_INSTANCE_LIFETIME_SECONDS": str(
            akamai["max_instance_lifetime_seconds"]
        ),
        "MCSERVER_R2_API_BASE_URL": "https://api.cloudflare.com/client/v4",
        "MCSERVER_R2_ACCOUNT_ID": r2["account_id"],
        "MCSERVER_R2_PARENT_ACCESS_KEY_ID": r2["parent_access_key_id"],
        "MCSERVER_R2_BUCKET": r2["bucket"],
        "MCSERVER_R2_TEMPORARY_CREDENTIAL_TTL_SECONDS": str(
            r2["temporary_credential_ttl_seconds"]
        ),
        "MCSERVER_R2_REQUEST_TIMEOUT_SECONDS": "30",
    }
    for key, value in values.items():
        string_value = str(value)
        if "\n" in string_value or "\r" in string_value or "\0" in string_value:
            raise DeployError(f"generated environment value {key} is invalid")
    return "".join(f"{key}={value}\n" for key, value in values.items())


def install_file(source: Path, destination: str, mode: str, group: str) -> None:
    destination_path = Path(destination)
    if source.resolve() == destination_path.resolve():
        run(["chown", f"root:{group}", "--", destination])
        run(["chmod", mode, "--", destination])
        return
    run(
        [
            "install",
            f"-m{mode}",
            "-o",
            "root",
            "-g",
            group,
            "--",
            str(source),
            destination,
        ]
    )


def extract_server_trust_anchor(
    fullchain: Path, ca_bundle: Path, destination: Path
) -> None:
    fullchain_bytes = fullchain.read_bytes()
    ca_bundle_bytes = ca_bundle.read_bytes()
    if (
        not fullchain_bytes
        or len(fullchain_bytes) > MAX_PKI_INPUT_BYTES
        or not ca_bundle_bytes
        or len(ca_bundle_bytes) > MAX_PKI_INPUT_BYTES
    ):
        raise DeployError("TLS certificate input is empty or exceeds one MiB")
    chain = PEM_CERTIFICATE.findall(fullchain_bytes)
    candidates = PEM_CERTIFICATE.findall(ca_bundle_bytes)
    if not chain or not candidates:
        raise DeployError("TLS certificate input contains no PEM certificate")

    with tempfile.TemporaryDirectory(prefix="mcserver-trust-") as raw_work:
        work = Path(raw_work)
        leaf = work / "leaf.pem"
        leaf.write_bytes(chain[0] + b"\n")
        intermediates = work / "intermediates.pem"
        if len(chain) > 1:
            intermediates.write_bytes(
                b"".join(certificate + b"\n" for certificate in chain[1:])
            )
        candidate_path = work / "candidate.pem"
        selected: bytes | None = None
        for candidate in candidates:
            candidate_path.write_bytes(candidate + b"\n")
            command = [
                "/usr/bin/openssl",
                "verify",
                "-purpose",
                "sslserver",
                "-CAfile",
                str(candidate_path),
            ]
            if intermediates.is_file():
                command.extend(["-untrusted", str(intermediates)])
            command.append(str(leaf))
            result = subprocess.run(
                command,
                check=False,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            if result.returncode == 0:
                selected = candidate + b"\n"
                break
        if selected is None:
            raise DeployError(
                "no certificate in files.remote_tls_root_ca validates "
                "files.remote_tls_fullchain"
            )
        destination.write_bytes(selected)
        destination.chmod(0o644)


def install_configuration(config: Config, environment: str) -> None:
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", prefix="mcserver-env-", delete=False
    ) as temporary:
        temporary.write(environment)
        environment_path = Path(temporary.name)
    try:
        environment_path.chmod(0o600)
        install_file(
            environment_path,
            "/etc/mcserver/control-plane.env",
            "0640",
            "mcserver",
        )
    finally:
        environment_path.unlink(missing_ok=True)

    secret_destinations = {
        "akamai_api_token": "akamai-api-token",
        "r2_api_token": "r2-api-token",
        "remote_tls_private_key": "remote-tls-private-key.pem",
        "agent_client_ca_private_key": "agent-client-ca-private-key.pem",
        "r2_runtime_environment": "r2-runtime.env",
    }
    for source_name, destination_name in secret_destinations.items():
        install_file(
            config.files[source_name],
            f"/etc/mcserver/credentials/{destination_name}",
            "0600",
            "root",
        )
    install_file(
        config.files["remote_tls_fullchain"],
        "/etc/mcserver/pki/remote-tls-fullchain.pem",
        "0644",
        "mcserver",
    )
    with tempfile.NamedTemporaryFile(
        prefix="mcserver-trust-anchor-", delete=False
    ) as temporary:
        trust_anchor = Path(temporary.name)
    try:
        extract_server_trust_anchor(
            config.files["remote_tls_fullchain"],
            config.files["remote_tls_root_ca"],
            trust_anchor,
        )
        install_file(
            trust_anchor,
            "/etc/mcserver/pki/remote-tls-root-ca.pem",
            "0644",
            "mcserver",
        )
    finally:
        trust_anchor.unlink(missing_ok=True)
    install_file(
        config.files["agent_client_ca_certificate"],
        "/etc/mcserver/pki/agent-client-ca.pem",
        "0644",
        "mcserver",
    )
    install_file(
        config.files["authorized_keys"],
        "/etc/mcserver/authorized_keys",
        "0640",
        "mcserver",
    )


def existing_live_creation_enabled(
    path: Path = Path("/etc/mcserver/control-plane.env"),
) -> bool:
    if not path.is_file():
        return False
    try:
        values = parse_environment_file(path)
    except (OSError, UnicodeError, DeployError):
        return False
    return values.get("MCSERVER_AKAMAI_LIVE_ENABLED") == "true"


def verify_almalinux() -> None:
    os_release: dict[str, str] = {}
    for raw in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
        key, separator, value = raw.partition("=")
        if separator:
            os_release[key] = value.strip().strip("\"'")
    if os_release.get("ID") != "almalinux":
        raise DeployError("production deploy requires AlmaLinux")
    if os_release.get("VERSION_ID", "").split(".", maxsplit=1)[0] != "10":
        raise DeployError("production deploy requires AlmaLinux 10")


def ping_response_is_ok(output: str) -> bool:
    try:
        response = json.loads(output)
    except json.JSONDecodeError:
        return False
    return isinstance(response, dict) and response.get("status") == "ok"


def wait_for_ping(timeout_seconds: float = 45) -> str:
    deadline = time.monotonic() + timeout_seconds
    last_output = ""
    while time.monotonic() < deadline:
        result = subprocess.run(
            [
                "/usr/local/bin/mcserverctl",
                "--socket",
                "/run/mcserver/control-plane.sock",
                "ping",
            ],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        last_output = result.stdout.strip()
        if result.returncode == 0 and ping_response_is_ok(last_output):
            return last_output
        time.sleep(1)
    raise DeployError(f"control-plane ping did not succeed: {last_output}")


def verify_service(config: Config, report: Report) -> None:
    run(["systemctl", "is-enabled", "mcserver-control-plane.service"])
    run(["systemctl", "is-active", "mcserver-control-plane.service"])
    report.record("control-plane ping", wait_for_ping())
    tls = run(
        [
            "/usr/bin/openssl",
            "s_client",
            "-connect",
            config.service["public_address"],
            "-servername",
            config.service["server_name"],
            "-CAfile",
            "/etc/mcserver/pki/remote-tls-root-ca.pem",
            "-verify_return_error",
        ],
        timeout=30,
    )
    if "Verify return code: 0 (ok)" not in tls.stdout:
        raise DeployError("external TLS verification did not return code 0")
    report.record("public TLS", config.service["public_address"])


def deploy(
    config: Config,
    package: Path,
    report: Report,
    *,
    go_live: bool,
) -> None:
    if os.geteuid() != 0:
        raise DeployError("deploy must run as root")
    verify_almalinux()
    report.record("host operating system", "AlmaLinux 10")
    restore_live_after_upgrade = existing_live_creation_enabled()

    run(["dnf", "install", "-y", "ca-certificates", "openssl", "systemd"], timeout=600)
    run(
        [
            str(package / "deploy/install-control-plane.sh"),
            str(package / "mcserver-control-plane"),
            str(package / "mcserverctl"),
        ],
        capture=False,
    )
    if shutil.which("certbot") is None:
        raise DeployError("certbot is required before production deployment")
    run(
        [
            str(package / "deploy/install-certbot-renewal-hook.sh"),
            config.service["certbot_lineage"],
        ],
        capture=False,
    )
    node_digest = sha256(package / "mcserver-node-agent")
    safe_environment = render_environment(config, node_digest, live=False)
    install_configuration(config, safe_environment)
    report.record("production configuration", "installed with live creation disabled")

    run(["systemctl", "enable", "mcserver-control-plane.service"])
    run(["systemctl", "restart", "mcserver-control-plane.service"])
    report.record("no-create preflight", wait_for_ping())
    verify_service(config, report)

    if not go_live and not restore_live_after_upgrade:
        return

    live_environment = render_environment(config, node_digest, live=True)
    install_configuration(config, live_environment)
    try:
        run(["systemctl", "restart", "mcserver-control-plane.service"])
        report.record("live control plane", wait_for_ping())
        if not go_live:
            report.record("live state after upgrade", "preserved")
            verify_service(config, report)
            return
        acceptance_script = (
            Path(__file__).resolve().parent.parent / "scripts/live_akamai_e2e.py"
        )
        if not acceptance_script.is_file():
            raise DeployError(
                "live acceptance script is missing; run production_deploy.py "
                "from a complete repository checkout"
            )
        run(
            [
                sys.executable,
                str(acceptance_script),
                "--confirm-billable-akamai-run",
                CONFIRMATION,
                "--accept-eula",
                "--socket",
                "/run/mcserver/control-plane.sock",
                "--firewall-id",
                str(config.acceptance["firewall_id"]),
                "--region",
                config.acceptance["region"],
                "--image",
                config.acceptance["image"],
                "--instance-type",
                config.acceptance["instance_type"],
                "--host-port",
                str(config.acceptance["host_port"]),
            ],
            timeout=require_positive_number(
                config.acceptance, "timeout_seconds", "acceptance"
            ),
            capture=False,
        )
        report.record("live two-generation acceptance")
        verify_service(config, report)
    except BaseException:
        install_configuration(config, safe_environment)
        run(["systemctl", "restart", "mcserver-control-plane.service"])
        raise


def main() -> int:
    args = parse_args()
    report = Report(command=args.command, config=str(args.config))
    try:
        if args.go_live and args.command != "deploy":
            raise DeployError("--go-live is valid only with the deploy command")
        if args.go_live and (
            args.confirm_billable_akamai_run != CONFIRMATION
            or not args.accept_minecraft_eula
        ):
            raise DeployError(
                "--go-live requires --accept-minecraft-eula and the exact "
                "--confirm-billable-akamai-run phrase"
            )

        config = load_config(args.config)
        report.release = config.release.version
        validate_config(config)
        report.record("deployment inputs")

        if args.command == "verify":
            verify_service(config, report)
        else:
            with tempfile.TemporaryDirectory(prefix="mcserver-production-") as raw_work:
                package = verify_release(config, Path(raw_work), report)
                if args.command == "deploy":
                    deploy(config, package, report, go_live=args.go_live)
        report.succeed()
        return_code = 0
    except KeyboardInterrupt:
        error = DeployError("production deployment interrupted")
        print(str(error), file=sys.stderr)
        report.fail(error)
        return_code = 130
    except Exception as error:
        print(f"production deployment failed: {error}", file=sys.stderr)
        report.fail(error)
        return_code = 1
    finally:
        report.write(args.report)
    return return_code


if __name__ == "__main__":
    raise SystemExit(main())
