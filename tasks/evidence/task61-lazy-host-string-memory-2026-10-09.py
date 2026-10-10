#!/usr/bin/env python3
"""Alternating production RSS/instruction screen for retained JS strings."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import re
import statistics
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = ROOT / "tasks/evidence"
BASELINE = ROOT / "target/iteration/task61-argument-shapes-profile-20261009/production/quench-node"
CANDIDATE = ROOT / "target/iteration/task61-lazy-host-string-20261009/production/quench-node"
CASES = {
    "short_concat": EVIDENCE / "task61-lazy-host-string-short-2026-10-09.cjs",
    "number_string": EVIDENCE / "task61-lazy-host-string-number-2026-10-09.cjs",
    "splay_payload": EVIDENCE / "task61-lazy-host-string-splay-2026-10-09.cjs",
    "slice": EVIDENCE / "task61-lazy-host-string-slice-2026-10-09.cjs",
}
INSTRUCTIONS = re.compile(r"^\s*(\d+)\s+instructions retired\s*$", re.MULTILINE)
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


def summarize(records: list[dict[str, object]]) -> dict[str, object]:
    summary = {}
    cases = sorted({str(record["case"]) for record in records})
    for case in cases:
        case_records = [record for record in records if record["case"] == case]
        variants = {
            variant: [record for record in case_records if record["variant"] == variant]
            for variant in ("baseline", "candidate")
        }
        metrics = {}
        for name in ("maximum_rss_bytes", "instructions_retired", "elapsed_seconds"):
            baseline_values = {int(record["pair"]): record[name] for record in variants["baseline"]}
            candidate_values = {int(record["pair"]): record[name] for record in variants["candidate"]}
            paired_changes = [
                (float(candidate_values[pair]) / float(baseline_values[pair]) - 1.0) * 100.0
                for pair in sorted(baseline_values.keys() & candidate_values.keys())
            ]
            baseline_median = statistics.median(baseline_values.values())
            candidate_median = statistics.median(candidate_values.values())
            metrics[name] = {
                "baseline_median": baseline_median,
                "candidate_median": candidate_median,
                "median_absolute_delta": candidate_median - baseline_median,
                "median_change_percent": (candidate_median / baseline_median - 1.0) * 100.0,
                "paired_median_change_percent": statistics.median(paired_changes),
                "candidate_lower_pairs": sum(change < 0 for change in paired_changes),
                "pairs": len(paired_changes),
            }
        summary[case] = {
            "metrics": metrics,
            "stdout_equal_every_pair": all(
                variants["baseline"][pair]["stdout"] == variants["candidate"][pair]["stdout"]
                for pair in range(len(variants["baseline"]))
            ),
        }
    return summary


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=11)
    parser.add_argument(
        "--out",
        type=Path,
        default=EVIDENCE / "task61-lazy-host-string-memory-2026-10-09.json",
    )
    args = parser.parse_args()
    rounds = args.rounds
    binaries = {"baseline": BASELINE, "candidate": CANDIDATE}
    for name, path in {**binaries, **CASES}.items():
        if not path.is_file():
            raise SystemExit(f"missing {name}: {path}")

    records = []
    for case, script in CASES.items():
        for pair in range(1, rounds + 1):
            order = ("baseline", "candidate") if pair % 2 else ("candidate", "baseline")
            outputs = {}
            for variant in order:
                command = ["/usr/bin/time", "-l", str(binaries[variant]), str(script)]
                started = time.monotonic()
                completed = subprocess.run(
                    command, cwd=ROOT, text=True, capture_output=True, check=False
                )
                outputs[variant] = completed.stdout
                records.append(
                    {
                        "case": case,
                        "pair": pair,
                        "order": list(order),
                        "variant": variant,
                        "command": command,
                        "exit_code": completed.returncode,
                        "stdout": completed.stdout,
                        "stderr": completed.stderr,
                        "wall_seconds": time.monotonic() - started,
                        "instructions_retired": metric(
                            INSTRUCTIONS, completed.stderr, "instructions_retired"
                        ),
                        "maximum_rss_bytes": metric(RSS, completed.stderr, "maximum_rss_bytes"),
                        "elapsed_seconds": metric(ELAPSED, completed.stderr, "elapsed_seconds"),
                    }
                )
                if completed.returncode != 0:
                    raise SystemExit(f"{variant}/{case} failed with {completed.returncode}")
            if outputs["baseline"] != outputs["candidate"]:
                raise SystemExit(f"stdout mismatch in {case} pair {pair}")

    report = {
        "task": 61,
        "experiment": "lazy derived UTF-8 host-string memory screen",
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "processor": platform.processor(),
            "python": platform.python_version(),
        },
        "rounds_per_case": rounds,
        "iterations_per_case": 200_000,
        "binary_sha256": {name: sha256(path) for name, path in binaries.items()},
        "input_sha256": {name: sha256(path) for name, path in CASES.items()},
        "summary": summarize(records),
        "records": records,
    }
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(args.out)
    print(f"records={len(records)}")


if __name__ == "__main__":
    main()
