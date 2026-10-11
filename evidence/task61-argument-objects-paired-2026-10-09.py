#!/usr/bin/env python3
"""Paired Quench-only argument-object allocation screen on macOS."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import re
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = ROOT / "tasks/evidence"
BASELINE = ROOT / "target/iteration/task61-local-bounds-prod/production/quench-node"
CANDIDATE = ROOT / "target/iteration/task61-argument-shapes-profile-20261009/production/quench-node"
CASES = {
    "plain": EVIDENCE / "task61-argument-objects-plain-2026-10-09.cjs",
    "mapped": EVIDENCE / "task61-argument-objects-mapped-2026-10-09.cjs",
    "unmapped": EVIDENCE / "task61-argument-objects-unmapped-2026-10-09.cjs",
}
INSTRUCTIONS = re.compile(r"^\s*(\d+)\s+instructions retired\s*$", re.MULTILINE)
CYCLES = re.compile(r"^\s*(\d+)\s+cycles elapsed\s*$", re.MULTILINE)
RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size\s*$", re.MULTILINE)
ELAPSED = re.compile(r"^\s*([0-9.]+)\s+real\b", re.MULTILINE)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def metric(pattern: re.Pattern[str], stderr: str, name: str) -> int | float:
    match = pattern.search(stderr)
    if match is None:
        raise RuntimeError(f"time -l did not report {name}: {stderr}")
    value = match.group(1)
    return float(value) if name == "elapsed_seconds" else int(value)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=11)
    parser.add_argument(
        "--out",
        type=Path,
        default=EVIDENCE / "task61-argument-objects-paired-2026-10-09.json",
    )
    args = parser.parse_args()

    binaries = {"baseline": BASELINE, "candidate": CANDIDATE}
    for name, path in {**binaries, **CASES}.items():
        if not path.is_file():
            raise SystemExit(f"missing {name}: {path}")

    records = []
    for case, script in CASES.items():
        for pair in range(1, args.rounds + 1):
            order = ("baseline", "candidate") if pair % 2 else ("candidate", "baseline")
            for variant in order:
                command = ["/usr/bin/time", "-l", str(binaries[variant]), str(script)]
                started = time.monotonic()
                completed = subprocess.run(
                    command,
                    cwd=ROOT,
                    text=True,
                    capture_output=True,
                    check=False,
                )
                elapsed_wall = time.monotonic() - started
                stderr = completed.stderr
                records.append(
                    {
                        "case": case,
                        "pair": pair,
                        "order": list(order),
                        "variant": variant,
                        "command": command,
                        "exit_code": completed.returncode,
                        "stdout": completed.stdout,
                        "stderr": stderr,
                        "wall_seconds": elapsed_wall,
                        "instructions_retired": metric(INSTRUCTIONS, stderr, "instructions_retired"),
                        "cycles_elapsed": metric(CYCLES, stderr, "cycles_elapsed"),
                        "maximum_rss_bytes": metric(RSS, stderr, "maximum_rss_bytes"),
                        "elapsed_seconds": metric(ELAPSED, stderr, "elapsed_seconds"),
                    }
                )
                if completed.returncode != 0:
                    raise SystemExit(f"{variant}/{case} failed with {completed.returncode}")

    report = {
        "task": 61,
        "experiment": "arguments-object template fast path",
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "processor": platform.processor(),
            "python": platform.python_version(),
        },
        "rounds_per_case": args.rounds,
        "iterations_per_case": 250000,
        "binary_sha256": {name: sha256(path) for name, path in binaries.items()},
        "input_sha256": {name: sha256(path) for name, path in CASES.items()},
        "records": records,
    }
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(args.out)
    print(f"records={len(records)}")


if __name__ == "__main__":
    main()
