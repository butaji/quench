"""Resolve and record the Node source used by native-codec build tools."""

import hashlib
import re
import subprocess
from pathlib import Path


def _node_version(node: Path, parser) -> tuple[str, Path]:
    header = node / "src/node_version.h"
    try:
        text = header.read_text()
    except OSError as error:
        parser.error(f"cannot read Node version header {header}: {error}")

    values = {}
    for name in ("MAJOR", "MINOR", "PATCH"):
        match = re.search(
            rf"^#define NODE_{name}_VERSION\s+(\d+)\s*$", text, re.MULTILINE
        )
        if match is None:
            parser.error(f"Node version header is missing NODE_{name}_VERSION")
        values[name] = match.group(1)

    return f"v{values['MAJOR']}.{values['MINOR']}.{values['PATCH']}", header


def resolve_node_source(
    node: Path,
    repo: Path,
    parser,
    *,
    release_source: bool,
    expected_node_version: str | None,
) -> dict[str, str]:
    """Require the submodule revision by default, or explicitly attest a release tree."""
    node_version, version_header = _node_version(node, parser)
    version_header_hash = hashlib.sha256(version_header.read_bytes()).hexdigest()

    if release_source:
        if expected_node_version is None:
            parser.error(
                "--release-source requires --expected-node-version (for example v26.12.1)"
            )
        if expected_node_version != node_version:
            parser.error(
                f"Node source version is {node_version}, expected {expected_node_version}"
            )
        if re.search(
            r"^#define NODE_VERSION_IS_RELEASE\s+1\s*$",
            version_header.read_text(),
            re.MULTILINE,
        ) is None:
            parser.error("--release-source requires NODE_VERSION_IS_RELEASE 1")
        if (node / ".git").exists():
            parser.error(
                "--release-source expects an extracted Node source tree without Git metadata"
            )
        return {
            "node_source_mode": "release-tree",
            "node_source": f"release:{node_version}",
            "node_version": node_version,
            "node_version_header_sha256": version_header_hash,
        }

    if expected_node_version is not None:
        parser.error("--expected-node-version is only valid with --release-source")
    try:
        expected_commit = subprocess.check_output(
            ["git", "ls-tree", "HEAD", "tests/node"], cwd=repo, text=True
        ).split()[2]
        actual_commit = subprocess.check_output(
            ["git", "-C", str(node), "rev-parse", "HEAD"], text=True
        ).strip()
    except (IndexError, subprocess.CalledProcessError) as error:
        parser.error(f"could not verify Node source against tests/node: {error}")
    if actual_commit != expected_commit:
        parser.error("Node checkout must match configured tests/node")
    return {
        "node_source_mode": "tests-node-submodule",
        "node_source": actual_commit,
        "node_version": node_version,
        "node_version_header_sha256": version_header_hash,
    }
