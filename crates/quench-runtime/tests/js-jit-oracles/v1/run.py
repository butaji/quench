#!/usr/bin/env python3
"""Run the focused JS JIT oracle inventory using only the Python standard library."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


STATS_PREFIX = "QUENCH_JS_JIT_STATS "


def repository_root() -> Path:
    for parent in Path(__file__).resolve().parents:
        if (parent / "crates" / "quench-runtime" / "Cargo.toml").is_file():
            return parent
    raise RuntimeError("could not locate Quench repository root")


def run(command: list[str], *, env: dict[str, str], timeout: int) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        timeout=timeout,
        check=False,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--node", default="node", help="Node.js oracle executable")
    parser.add_argument("--quench", help="Quench executable (defaults to target/iteration/quench-node)")
    parser.add_argument("--skip-build", action="store_true", help="do not build a missing Quench executable")
    parser.add_argument(
        "--allow-node-version-mismatch",
        action="store_true",
        help="run against a non-pinned Node version; results are diagnostic, not pinned-oracle evidence",
    )
    parser.add_argument("--timeout-secs", type=int, default=60)
    parser.add_argument("--build-timeout-secs", type=int, default=900)
    args = parser.parse_args()

    root = repository_root()
    base = Path(__file__).resolve().parent
    manifest_path = base / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    fixtures_dir = base / manifest["fixture_root"]
    build_config = manifest["quench_build"]
    build_profile = build_config["profile"]

    node_env = os.environ.copy()
    node = run([args.node, "--version"], env=node_env, timeout=args.timeout_secs)
    if node.returncode != 0:
        print(f"failed to query Node version: {node.stderr.strip()}", file=sys.stderr)
        return 2
    node_version = node.stdout.strip()
    expected_node = manifest["oracle"]["version"]
    if node_version != expected_node and not args.allow_node_version_mismatch:
        print(
            f"oracle requires Node {expected_node}, found {node_version}; "
            "use --allow-node-version-mismatch only for diagnostic runs",
            file=sys.stderr,
        )
        return 2

    quench = Path(args.quench).resolve() if args.quench else root / "target" / build_profile / "quench-node"
    build_env = os.environ.copy()
    build_env.update(build_config["build_environment"])
    if args.skip_build:
        if not quench.is_file():
            print(f"Quench binary does not exist: {quench}", file=sys.stderr)
            return 2
    else:
        build = run(
            ["cargo", "build", "--locked", "--profile", build_profile, "-p", "quench-node", "--bin", "quench-node"],
            env=build_env,
            timeout=args.build_timeout_secs,
        )
        if build.returncode != 0:
            sys.stderr.write(build.stdout)
            sys.stderr.write(build.stderr)
            return build.returncode
    if not quench.is_file():
        print(f"Quench build succeeded but binary is missing: {quench}", file=sys.stderr)
        return 2

    quench_env = os.environ.copy()
    quench_env.update(manifest["jit_environment"])
    revision = run(["git", "rev-parse", "HEAD"], env=build_env, timeout=args.timeout_secs)
    quench_revision = revision.stdout.strip() if revision.returncode == 0 else "unknown"
    print(f"Oracle Node {node_version}; Quench source {quench_revision}; profile {build_profile} (--locked)")
    failed = False
    for fixture in manifest["fixtures"]:
        path = fixtures_dir / fixture["path"]
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != fixture["sha256"]:
            print(f"FAIL {fixture['id']}: fixture SHA-256 mismatch ({digest})", file=sys.stderr)
            failed = True
            continue

        oracle = run([args.node, str(path)], env=node_env, timeout=args.timeout_secs)
        if (oracle.returncode, oracle.stdout, oracle.stderr) != (
            fixture["expected"]["exit_code"],
            fixture["expected"]["stdout"],
            fixture["expected"]["stderr"],
        ):
            print(f"FAIL {fixture['id']}: pinned oracle output differs from manifest", file=sys.stderr)
            print(f"  actual exit={oracle.returncode} stdout={oracle.stdout!r} stderr={oracle.stderr!r}", file=sys.stderr)
            failed = True
            continue

        interpreter = run([str(quench), str(path)], env=quench_env, timeout=args.timeout_secs)
        interpreter_stderr = "\n".join(
            line for line in interpreter.stderr.splitlines() if not line.startswith("QUENCH_JS_JIT_")
        )
        if interpreter_stderr:
            interpreter_stderr += "\n"
        if (interpreter.returncode, interpreter.stdout, interpreter_stderr) != (
            oracle.returncode,
            oracle.stdout,
            oracle.stderr,
        ):
            print(f"FAIL {fixture['id']}: interpreter differs from Node oracle", file=sys.stderr)
            print(f"  interpreter exit={interpreter.returncode} stdout={interpreter.stdout!r} stderr={interpreter_stderr!r}", file=sys.stderr)
            failed = True

        jit = run([str(quench), "--jit", str(path)], env=quench_env, timeout=args.timeout_secs)
        jit_stderr = "\n".join(line for line in jit.stderr.splitlines() if not line.startswith("QUENCH_JS_JIT_"))
        if jit_stderr:
            jit_stderr += "\n"
        if (jit.returncode, jit.stdout, jit_stderr) != (oracle.returncode, oracle.stdout, oracle.stderr):
            print(f"FAIL {fixture['id']}: --jit result differs from Node oracle", file=sys.stderr)
            print(f"  jit exit={jit.returncode} stdout={jit.stdout!r} stderr={jit_stderr!r}", file=sys.stderr)
            failed = True
        try:
            stat_rows = [json.loads(line[len(STATS_PREFIX) :]) for line in jit.stderr.splitlines() if line.startswith(STATS_PREFIX)]
        except json.JSONDecodeError as error:
            print(f"FAIL {fixture['id']}: malformed JIT stats: {error}", file=sys.stderr)
            failed = True
            continue
        native_rows = [row for row in stat_rows if row.get("native_entries", 0) > 0]
        native_names = {row.get("function_name", "") for row in native_rows}
        required_native = set(fixture["native_functions"])
        if not native_rows:
            print(f"FAIL {fixture['id']}: no native entries reported", file=sys.stderr)
            failed = True
        elif not required_native.issubset(native_names):
            print(
                f"FAIL {fixture['id']}: required native functions {sorted(required_native)}; "
                f"observed {sorted(name for name in native_names if name)}",
                file=sys.stderr,
            )
            failed = True
        else:
            total_native = sum(row.get("native_entries", 0) for row in native_rows)
            print(f"PASS {fixture['id']}: native_entries={total_native} native={','.join(sorted(required_native))}")

    if failed:
        return 1
    print(f"PASS all {len(manifest['fixtures'])} fixtures with Node {node_version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
