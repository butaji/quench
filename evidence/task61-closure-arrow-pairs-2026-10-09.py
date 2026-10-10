#!/usr/bin/env python3
"""Run alternating production instruction/RSS pairs for arrow-closure creation."""

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
FIELDS = (
    "pair", "order", "baseline_instructions", "candidate_instructions",
    "baseline_max_rss_bytes", "candidate_max_rss_bytes",
    "baseline_elapsed_seconds", "candidate_elapsed_seconds",
    "baseline_user_seconds", "candidate_user_seconds",
    "baseline_system_seconds", "candidate_system_seconds",
)


def measure(binary: Path, fixture: Path) -> dict[str, int | float]:
    completed = subprocess.run(
        [*TIME_COMMAND, str(binary), str(fixture)],
        text=True,
        capture_output=True,
        check=True,
    )
    report = completed.stderr
    instructions = int(INSTRUCTIONS_RE.search(report).group(1))
    max_rss = int(RSS_RE.search(report).group(1))
    elapsed, user, system = map(float, ELAPSED_RE.search(report).groups())
    return {
        "instructions": instructions,
        "max_rss_bytes": max_rss,
        "elapsed_seconds": elapsed,
        "user_seconds": user,
        "system_seconds": system,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--fixture", type=Path, default=Path(__file__).with_name("task61-closure-arrow-10m-2026-10-09.cjs"))
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    rows = []
    for pair in range(1, PAIR_COUNT + 1):
        order = ("candidate", "baseline") if pair % 2 else ("baseline", "candidate")
        samples = {
            label: measure(args.candidate if label == "candidate" else args.baseline, args.fixture)
            for label in order
        }
        rows.append({
            "pair": pair,
            "order": ">".join(order),
            **{
                f"{label}_{field}": value
                for label, sample in samples.items()
                for field, value in sample.items()
            },
        })
        print(f"pair {pair}/{PAIR_COUNT} complete", flush=True)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=FIELDS)
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    main()
