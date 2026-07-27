#!/usr/bin/env python3
"""Generate a deterministic SPDX 2.3 dependency document from Cargo metadata."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any
from urllib.parse import quote


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--name", required=True)
    parser.add_argument("--namespace", required=True)
    return parser.parse_args()


def cargo_metadata() -> dict[str, Any]:
    completed = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    return json.loads(completed.stdout)


def spdx_id(package_id: str) -> str:
    digest = hashlib.sha256(package_id.encode()).hexdigest()[:20]
    return f"SPDXRef-Package-{digest}"


def source_location(package: dict[str, Any]) -> str:
    source = package.get("source")
    if isinstance(source, str) and source.startswith("registry+"):
        return source.removeprefix("registry+")
    repository = package.get("repository")
    if isinstance(repository, str) and repository:
        return repository
    return "NOASSERTION"


def license_value(package: dict[str, Any]) -> str:
    value = package.get("license")
    if isinstance(value, str) and value.strip():
        return value
    return "NOASSERTION"


def created_timestamp() -> str:
    epoch = os.environ.get("SOURCE_DATE_EPOCH")
    if epoch is None:
        instant = dt.datetime.now(dt.UTC)
    else:
        instant = dt.datetime.fromtimestamp(int(epoch), tz=dt.UTC)
    return instant.replace(microsecond=0).isoformat().replace("+00:00", "Z")


def main() -> int:
    args = parse_args()
    metadata = cargo_metadata()
    packages = metadata["packages"]
    package_by_id = {package["id"]: package for package in packages}
    workspace_members = set(metadata["workspace_members"])
    ids = {package_id: spdx_id(package_id) for package_id in package_by_id}

    document_packages = []
    for package_id in sorted(package_by_id):
        package = package_by_id[package_id]
        name = package["name"]
        version = package["version"]
        document_packages.append(
            {
                "SPDXID": ids[package_id],
                "name": name,
                "versionInfo": version,
                "downloadLocation": source_location(package),
                "filesAnalyzed": False,
                "licenseConcluded": "NOASSERTION",
                "licenseDeclared": license_value(package),
                "copyrightText": "NOASSERTION",
                "externalRefs": [
                    {
                        "referenceCategory": "PACKAGE-MANAGER",
                        "referenceType": "purl",
                        "referenceLocator": (
                            f"pkg:cargo/{quote(name, safe='')}@{quote(version, safe='')}"
                        ),
                    }
                ],
            }
        )

    relationships: list[dict[str, str]] = []
    for package_id in sorted(workspace_members):
        relationships.append(
            {
                "spdxElementId": "SPDXRef-DOCUMENT",
                "relationshipType": "DESCRIBES",
                "relatedSpdxElement": ids[package_id],
            }
        )
    resolve = metadata.get("resolve") or {}
    for node in sorted(resolve.get("nodes", []), key=lambda value: value["id"]):
        source_id = node["id"]
        for dependency in sorted(node.get("deps", []), key=lambda value: value["pkg"]):
            dependency_id = dependency["pkg"]
            if source_id in ids and dependency_id in ids:
                relationships.append(
                    {
                        "spdxElementId": ids[source_id],
                        "relationshipType": "DEPENDS_ON",
                        "relatedSpdxElement": ids[dependency_id],
                    }
                )

    if not re.fullmatch(r"https://[^\s]+", args.namespace):
        raise ValueError("--namespace must be an absolute HTTPS URI")
    document = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": args.name,
        "documentNamespace": args.namespace,
        "creationInfo": {
            "created": created_timestamp(),
            "creators": ["Tool: minecraft-server-management/scripts/generate_spdx_sbom.py"],
        },
        "packages": document_packages,
        "relationships": relationships,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"SBOM generation failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
