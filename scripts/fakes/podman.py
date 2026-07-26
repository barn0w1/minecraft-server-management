#!/usr/bin/env python3
"""Small deterministic Podman substitute for process-level integration tests."""

from __future__ import annotations

import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from typing import Any, Iterator

RUNTIME_ENV = "MCSERVER_FAKE_RUNTIME_DIRECTORY"
STATE_FILE = "containers.json"
LOCK_FILE = "containers.lock"


def fail(message: str, code: int = 125) -> int:
    print(message, file=sys.stderr)
    return code


def runtime_directory() -> Path:
    value = os.environ.get(RUNTIME_ENV)
    if not value:
        raise RuntimeError(f"{RUNTIME_ENV} is required")
    directory = Path(value)
    directory.mkdir(parents=True, exist_ok=True)
    return directory


class State:
    def __init__(self) -> None:
        self.directory = runtime_directory()
        self.path = self.directory / STATE_FILE
        self.lock_path = self.directory / LOCK_FILE
        self.lock_file = None
        self.value: dict[str, Any] = {}

    def __enter__(self) -> "State":
        self.lock_file = self.lock_path.open("a+")
        fcntl.flock(self.lock_file.fileno(), fcntl.LOCK_EX)
        if self.path.exists():
            self.value = json.loads(self.path.read_text())
        else:
            self.value = {"containers": {}}
        self.value.setdefault("containers", {})
        return self

    def save(self) -> None:
        temporary = self.path.with_suffix(".tmp")
        temporary.write_text(json.dumps(self.value, indent=2, sort_keys=True) + "\n")
        os.replace(temporary, self.path)

    def __exit__(self, exc_type: object, exc: object, traceback: object) -> None:
        if exc_type is None:
            self.save()
        assert self.lock_file is not None
        fcntl.flock(self.lock_file.fileno(), fcntl.LOCK_UN)
        self.lock_file.close()


def option_values(arguments: list[str], name: str) -> Iterator[str]:
    index = 0
    while index < len(arguments):
        if arguments[index] == name and index + 1 < len(arguments):
            yield arguments[index + 1]
            index += 2
        else:
            index += 1


def labels_match(container: dict[str, Any], filters: list[str]) -> bool:
    labels: dict[str, str] = container.get("labels", {})
    for value in filters:
        if not value.startswith("label="):
            continue
        expression = value.removeprefix("label=")
        if "=" in expression:
            key, expected = expression.split("=", 1)
            if labels.get(key) != expected:
                return False
        elif expression not in labels:
            return False
    return True


def command_ps(arguments: list[str]) -> int:
    filters = list(option_values(arguments, "--filter"))
    quiet = "--quiet" in arguments or "-q" in arguments
    formats = list(option_values(arguments, "--format"))
    output_format = formats[-1] if formats else None
    with State() as state:
        containers = state.value["containers"]
        for name in sorted(containers):
            container = containers[name]
            if not labels_match(container, filters):
                continue
            if quiet:
                print(name)
            elif output_format is not None:
                if "io.mcserver.server-instance-id" in output_format:
                    labels = container.get("labels", {})
                    instance_id = labels.get("io.mcserver.server-instance-id", "")
                    if "io.mcserver.compute-instance-id" in output_format:
                        compute_id = labels.get("io.mcserver.compute-instance-id", "")
                        print(f"{name}|{instance_id}|{compute_id}")
                    else:
                        print(f"{name}|{instance_id}")
                else:
                    return fail(f"unsupported fake podman format: {output_format}")
            else:
                print(name)
    return 0


def command_create(arguments: list[str]) -> int:
    try:
        separator = arguments.index("--")
    except ValueError:
        separator = len(arguments) - 1
    options = arguments[:separator]
    image = arguments[separator + 1] if separator + 1 < len(arguments) else ""
    names = list(option_values(options, "--name"))
    if not names:
        return fail("fake podman create requires --name")
    name = names[-1]
    labels: dict[str, str] = {}
    for label in option_values(options, "--label"):
        key, separator, value = label.partition("=")
        if not separator:
            return fail(f"invalid label: {label}")
        labels[key] = value
    volumes = list(option_values(options, "--volume"))
    data_directory = None
    if volumes:
        data_directory = volumes[-1].split(":", 1)[0]
    publishes = list(option_values(options, "--publish"))
    with State() as state:
        containers = state.value["containers"]
        containers[name] = {
            "name": name,
            "image": image,
            "labels": labels,
            "running": False,
            "data_directory": data_directory,
            "publish": publishes[-1] if publishes else None,
        }
    return 0


def command_start(arguments: list[str]) -> int:
    if len(arguments) != 1:
        return fail("fake podman start requires one container")
    name = arguments[0]
    with State() as state:
        container = state.value["containers"].get(name)
        if container is None:
            return fail(f"no such container: {name}", 1)
        container["running"] = True
        data_directory = container.get("data_directory")
        if data_directory:
            data = Path(data_directory)
            data.mkdir(parents=True, exist_ok=True)
            (data / "fake-minecraft-state.txt").write_text(
                f"container={name}\nstarted=true\n"
            )
    return 0


def command_stop(arguments: list[str]) -> int:
    names = [argument for argument in arguments if not argument.startswith("-")]
    if "--time" in arguments:
        timeout_index = arguments.index("--time")
        names = arguments[timeout_index + 2 :]
    if len(names) != 1:
        return fail("fake podman stop requires one container")
    with State() as state:
        container = state.value["containers"].get(names[0])
        if container is None:
            return fail(f"no such container: {names[0]}", 1)
        container["running"] = False
    return 0


def command_remove(arguments: list[str]) -> int:
    names = [argument for argument in arguments if not argument.startswith("-")]
    ignore = "--ignore" in arguments
    with State() as state:
        containers = state.value["containers"]
        missing = [name for name in names if name not in containers]
        for name in names:
            containers.pop(name, None)
        if missing and not ignore:
            return fail(f"no such container: {missing[0]}", 1)
    return 0


def command_container(arguments: list[str]) -> int:
    if len(arguments) >= 2 and arguments[0] == "exists":
        with State() as state:
            return 0 if arguments[1] in state.value["containers"] else 1
    if len(arguments) >= 4 and arguments[0] == "inspect":
        name = arguments[-1]
        with State() as state:
            container = state.value["containers"].get(name)
            if container is None:
                return fail(f"no such container: {name}", 1)
            print("true" if container.get("running") else "false")
            return 0
    return fail(f"unsupported fake podman container command: {arguments!r}")


def command_unshare(arguments: list[str]) -> int:
    if not arguments:
        return fail("fake podman unshare requires a command")
    command = arguments[0]
    rest = arguments[1:]
    if command == "rm":
        paths = [Path(value) for value in rest if value not in {"-r", "-f", "-rf", "--"}]
        for path in paths:
            if path.is_dir() and not path.is_symlink():
                shutil.rmtree(path, ignore_errors=True)
            else:
                try:
                    path.unlink()
                except FileNotFoundError:
                    pass
        return 0
    if command == "mv":
        paths = [Path(value) for value in rest if value != "--"]
        if len(paths) != 2:
            return fail("fake podman unshare mv requires source and destination")
        source, destination = paths
        destination.parent.mkdir(parents=True, exist_ok=True)
        os.replace(source, destination)
        return 0
    completed = subprocess.run([command, *rest], check=False)
    return completed.returncode



def should_fail_once(command: str) -> bool:
    configured = {
        item.strip()
        for item in os.environ.get("MCSERVER_FAKE_PODMAN_FAIL_ONCE", "").split(",")
        if item.strip()
    }
    if command not in configured:
        return False
    marker = runtime_directory() / f"podman-failed-once-{command}"
    try:
        marker.open("x").close()
    except FileExistsError:
        return False
    return True

def main() -> int:
    arguments = sys.argv[1:]
    if not arguments:
        return fail("fake podman requires a command")
    command, rest = arguments[0], arguments[1:]
    try:
        if should_fail_once(command):
            return fail(f"injected one-time fake Podman failure for {command}")
        if command == "ps":
            return command_ps(rest)
        if command == "create":
            return command_create(rest)
        if command == "start":
            return command_start(rest)
        if command == "stop":
            return command_stop(rest)
        if command == "rm":
            return command_remove(rest)
        if command == "container":
            return command_container(rest)
        if command == "unshare":
            return command_unshare(rest)
        if command == "version":
            print("fake-podman 1.0")
            return 0
        return fail(f"unsupported fake podman command: {command} {rest!r}")
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        return fail(f"fake podman failed: {error}")


if __name__ == "__main__":
    raise SystemExit(main())
