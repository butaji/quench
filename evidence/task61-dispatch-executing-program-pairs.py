#!/usr/bin/env python3
"""Run alternating M4 instruction/RSS pairs for the dispatch program-id experiment."""

import argparse
import csv
import re
import subprocess
from pathlib import Path

PAIR_COUNT = 11
TIME_COMMAND = ("/usr/bin/time", "-l")
INSTRUCTIONS_RE = re.compile(r"(?m)^\s*(\d+)\s+instructions retired\s*$")
RSS_RE = re.compile(r"(?m)^\s*(\d+)\s+maximum resident set size\s*$")
ELAPSED_RE = re.compile(r"(?m)^\s*([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys\s*$")
FIXTURES = {
    "local_store": "task61-dispatch-executing-program-local-store-10m-2026-10-08.cjs",
    "cjs_call": "task61-dispatch-executing-program-cjs-call-10m-2026-10-08.cjs",
}
FIELDS = (
    "fixture", "pair", "order", "baseline_instructions", "candidate_instructions",
    "baseline_elapsed_seconds", "candidate_elapsed_seconds", "baseline_user_seconds",
    "candidate_user_seconds", "baseline_max_rss_bytes", "candidate_max_rss_bytes",
)


def measure(binary: Path, fixture: Path) -> dict[str, int | float]:
    completed = subprocess.run(
        [*TIME_COMMAND, str(binary), str(fixture)], text=True, capture_output=True, check=True
    )
    report = completed.stderr
    instructions = int(INSTRUCTIONS_RE.search(report).group(1))
    max_rss = int(RSS_RE.search(report).group(1))
    elapsed, user, _system = map(float, ELAPSED_RE.search(report).groups())
    return {
        "instructions": instructions,
        "elapsed_seconds": elapsed,
        "user_seconds": user,
        "max_rss_bytes": max_rss,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    evidence_dir = Path(__file__).resolve().parent
    rows = []
    for fixture_name, filename in FIXTURES.items():
        fixture = evidence_dir / filename
        for pair in range(1, PAIR_COUNT + 1):
            order = ("candidate", "baseline") if pair % 2 else ("baseline", "candidate")
            samples = {
                label: measure(args.candidate if label == "candidate" else args.baseline, fixture)
                for label in order
            }
            rows.append({
                "fixture": fixture_name,
                "pair": pair,
                "order": ">".join(order),
                **{f"baseline_{key}": value for key, value in samples["baseline"].items()},
                **{f"candidate_{key}": value for key, value in samples["candidate"].items()},
            })
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=FIELDS)
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    main()
