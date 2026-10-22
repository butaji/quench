#!/usr/bin/env python3
"""Run alternating production pairs for register-entry initialization."""

from __future__ import annotations

import argparse
import csv
import hashlib
import re
import subprocess
from pathlib import Path


INSTRUCTIONS = re.compile(r"(?m)^\s*(\d+)\s+instructions retired\s*$")
CYCLES = re.compile(r"(?m)^\s*(\d+)\s+cycles elapsed\s*$")
MAX_RSS = re.compile(r"(?m)^\s*(\d+)\s+maximum resident set size\s*$")
ELAPSED = re.compile(r"(?m)^\s*([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys\s*$")


def metric(pattern: re.Pattern[str], report: str, name: str) -> str:
    match = pattern.search(report)
    if match is None:
        raise RuntimeError(f"/usr/bin/time -l did not report {name}")
    return match.group(1)


def measure(binary: Path, fixture: Path) -> dict[str, str]:
    command = ["/usr/bin/time", "-l", str(binary), str(fixture)]
    completed = subprocess.run(command, capture_output=True, text=True, check=False)
    if completed.returncode != 0:
        raise RuntimeError(f"{binary} exited {completed.returncode}: {completed.stderr}")
    real, user, system = ELAPSED.search(completed.stderr).groups()
    return {
        "instructions_retired": metric(INSTRUCTIONS, completed.stderr, "instructions retired"),
        "cycles_elapsed": metric(CYCLES, completed.stderr, "cycles elapsed"),
        "elapsed_seconds": real,
        "user_seconds": user,
        "system_seconds": system,
        "maximum_rss_bytes": metric(MAX_RSS, completed.stderr, "maximum resident set size"),
        "stdout_sha256": hashlib.sha256(completed.stdout.encode()).hexdigest(),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--pairs", type=int, default=11)
    args = parser.parse_args()
    if args.pairs < 1:
        parser.error("--pairs must be positive")

    rows = []
    for pair in range(1, args.pairs + 1):
        order = ("candidate", "baseline") if pair % 2 else ("baseline", "candidate")
        binaries = {"baseline": args.baseline, "candidate": args.candidate}
        samples = {name: measure(binaries[name], args.fixture) for name in order}
        rows.append(
            {
                "fixture": args.fixture.name,
                "pair": pair,
                "order": ">".join(order),
                **{
                    f"{side}_{metric_name}": value
                    for side, sample in samples.items()
                    for metric_name, value in sample.items()
                },
            }
        )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=rows[0], lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    main()
