#!/usr/bin/env python3
"""Run 11 alternating M4 pairs for the long local-store loop."""

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


def measure(binary: Path, fixture: Path) -> dict[str, int | float]:
    completed = subprocess.run(
        [*TIME_COMMAND, str(binary), str(fixture)],
        text=True,
        capture_output=True,
        check=True,
    )
    report = completed.stderr
    elapsed, user, system = map(float, ELAPSED_RE.search(report).groups())
    return {
        "instructions": int(INSTRUCTIONS_RE.search(report).group(1)),
        "elapsed_seconds": elapsed,
        "user_seconds": user,
        "system_seconds": system,
        "max_rss_bytes": int(RSS_RE.search(report).group(1)),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    rows = []
    for pair in range(1, PAIR_COUNT + 1):
        order = ("candidate", "baseline") if pair % 2 else ("baseline", "candidate")
        samples = {
            label: measure(args.candidate if label == "candidate" else args.baseline, args.fixture)
            for label in order
        }
        rows.append(
            {
                "fixture": args.fixture.name,
                "pair": pair,
                "order": ">".join(order),
                **{
                    f"{side}_{metric}": value
                    for side, sample in samples.items()
                    for metric, value in sample.items()
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
