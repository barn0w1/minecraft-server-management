#!/usr/bin/env python3
"""Small deterministic restic substitute for process-level integration tests."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
from typing import Any


def fail(message: str, code: int = 1) -> int:
    print(message, file=sys.stderr)
    return code


def strip_global_options(arguments: list[str]) -> list[str]:
    result = list(arguments)
    while result[:1] == ["--retry-lock"] and len(result) >= 2:
        result = result[2:]
    return result


def normalize_repository(value: str) -> tuple[Path, bool]:
    if value.startswith("s3:https://"):
        runtime_value = os.environ.get("MCSERVER_FAKE_RUNTIME_DIRECTORY")
        if not runtime_value:
            raise ValueError("remote fake restic repository requires MCSERVER_FAKE_RUNTIME_DIRECTORY")
        digest = hashlib.sha256(value.encode()).hexdigest()
        return Path(runtime_value) / "restic-repositories" / digest, True
    return Path(value), False


def repository_from(arguments: list[str]) -> tuple[Path, list[str], bool]:
    arguments = list(arguments)
    for option in ("--repo", "-r"):
        if option in arguments:
            index = arguments.index(option)
            if index + 1 >= len(arguments):
                raise ValueError(f"{option} requires a value")
            repository, remote = normalize_repository(arguments[index + 1])
            del arguments[index : index + 2]
            return repository, arguments, remote
    value = os.environ.get("RESTIC_REPOSITORY")
    if not value:
        raise ValueError("RESTIC_REPOSITORY or --repo is required")
    repository, remote = normalize_repository(value)
    return repository, arguments, remote


def validate_remote_credentials() -> None:
    required = (
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AWS_DEFAULT_REGION",
    )
    missing = [name for name in required if not os.environ.get(name)]
    if missing:
        raise ValueError(f"remote fake restic credentials are incomplete: {missing!r}")
    if os.environ["AWS_DEFAULT_REGION"] != "auto":
        raise ValueError("Cloudflare R2 requires AWS_DEFAULT_REGION=auto")


def metadata_path(repository: Path) -> Path:
    return repository / "fake-restic.json"


def load_metadata(repository: Path) -> dict[str, Any]:
    path = metadata_path(repository)
    if not path.exists():
        raise FileNotFoundError("repository is not initialized")
    return json.loads(path.read_text())


def save_metadata(repository: Path, value: dict[str, Any]) -> None:
    path = metadata_path(repository)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    os.replace(temporary, path)


def hash_directory(path: Path, generation: int) -> str:
    digest = hashlib.sha256(str(generation).encode())
    if path.exists():
        for child in sorted(path.rglob("*"), key=lambda item: item.as_posix()):
            relative = child.relative_to(path).as_posix().encode()
            digest.update(relative)
            if child.is_file():
                digest.update(child.read_bytes())
    return digest.hexdigest()


def command_init(repository: Path) -> int:
    repository.mkdir(parents=True, exist_ok=True)
    (repository / "config").write_text("fake-restic-repository\n")
    (repository / "snapshots").mkdir(exist_ok=True)
    save_metadata(repository, {"generation": 0, "snapshots": []})
    return 0


def command_cat(repository: Path, arguments: list[str]) -> int:
    if arguments != ["config"]:
        return fail(f"unsupported fake restic cat command: {arguments!r}")
    if not (repository / "config").exists():
        return fail("repository is not initialized")
    print((repository / "config").read_text(), end="")
    return 0


def command_backup(repository: Path, arguments: list[str]) -> int:
    if not arguments:
        return fail("fake restic backup requires a path")
    source = Path(arguments[0])
    if not source.is_absolute():
        source = Path.cwd() / source
    metadata = load_metadata(repository)
    generation = int(metadata.get("generation", 0)) + 1
    snapshot_id = hash_directory(source, generation)
    snapshot_directory = repository / "snapshots" / snapshot_id
    if snapshot_directory.exists():
        shutil.rmtree(snapshot_directory)
    snapshot_directory.mkdir(parents=True)
    if source.exists():
        shutil.copytree(source, snapshot_directory / source.name)
    else:
        return fail(f"backup source does not exist: {source}")
    metadata["generation"] = generation
    metadata.setdefault("snapshots", []).append(snapshot_id)
    save_metadata(repository, metadata)
    print(json.dumps({"message_type": "summary", "snapshot_id": snapshot_id}))
    return 0


def command_restore(repository: Path, arguments: list[str]) -> int:
    if not arguments:
        return fail("fake restic restore requires a snapshot id")
    snapshot_id = arguments[0]
    try:
        target_index = arguments.index("--target")
        target = Path(arguments[target_index + 1])
    except (ValueError, IndexError):
        return fail("fake restic restore requires --target")
    source = repository / "snapshots" / snapshot_id
    if not source.is_dir():
        return fail(f"snapshot does not exist: {snapshot_id}")
    target.mkdir(parents=True, exist_ok=True)
    for child in source.iterdir():
        destination = target / child.name
        if child.is_dir():
            shutil.copytree(child, destination)
        else:
            shutil.copy2(child, destination)
    return 0



def should_fail_once(command: str) -> bool:
    configured = {
        item.strip()
        for item in os.environ.get("MCSERVER_FAKE_RESTIC_FAIL_ONCE", "").split(",")
        if item.strip()
    }
    if command not in configured:
        return False
    runtime_value = os.environ.get("MCSERVER_FAKE_RUNTIME_DIRECTORY")
    if not runtime_value:
        return False
    marker = Path(runtime_value) / f"restic-failed-once-{command}"
    marker.parent.mkdir(parents=True, exist_ok=True)
    try:
        marker.open("x").close()
    except FileExistsError:
        return False
    return True

def main() -> int:
    try:
        repository, arguments, remote = repository_from(strip_global_options(sys.argv[1:]))
        if remote:
            validate_remote_credentials()
        if not arguments:
            return fail("fake restic requires a command")
        command, rest = arguments[0], arguments[1:]
        if should_fail_once(command):
            return fail(f"injected one-time fake restic failure for {command}")
        if command == "init":
            return command_init(repository)
        if command == "cat":
            return command_cat(repository, rest)
        if command == "backup":
            return command_backup(repository, rest)
        if command == "restore":
            return command_restore(repository, rest)
        if command == "version":
            print("fake-restic 1.0")
            return 0
        return fail(f"unsupported fake restic command: {command} {rest!r}")
    except (OSError, ValueError, json.JSONDecodeError) as error:
        return fail(f"fake restic failed: {error}")


if __name__ == "__main__":
    raise SystemExit(main())
