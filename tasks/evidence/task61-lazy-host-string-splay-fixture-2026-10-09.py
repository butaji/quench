#!/usr/bin/env python3
"""Matched Quench-only Splay fixture check for lazy string views."""

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
TARGET = ROOT / "target/iteration/task61-lazy-host-string-20261009"
BASELINE = ROOT / "target/iteration/task61-argument-shapes-profile-20261009/production/quench-node"
CANDIDATE = TARGET / "production/quench-node"
SUITE = ROOT / "quench-bench/js-engine-benchmark/v8-v7"
RUNNER_SOURCE = ROOT / "quench-bench/src/main.rs"
RUNNER_PATTERN = re.compile(r'const RUNNER: &str = r#"(.*?)"#;', re.DOTALL)
SCORE = re.compile(r"(?m)^Score: ([0-9]+(?:\.[0-9]+)?)\s*$")
INSTRUCTIONS = re.compile(r"^\s*(\d+)\s+instructions retired\s*$", re.MULTILINE)
RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size\s*$", re.MULTILINE)
ELAPSED = re.compile(r"^\s*([0-9.]+)\s+real\b", re.MULTILINE)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def metric(pattern: re.Pattern[str], text: str, name: str) -> int | float:
    match = pattern.search(text)
    if match is None:
        raise RuntimeError(f"time -l did not report {name}: {text}")
    value = match.group(1)
    return float(value) if name == "elapsed_seconds" else int(value)


def semantic_output(stdout: str) -> str:
    return "\n".join(
        line
        for line in stdout.splitlines()
        if not line.startswith("Score: ")
        and line != "----"
        and not line.startswith("__quenchBenchResult: ")
    )


def summarize(records: list[dict[str, object]]) -> dict[str, object]:
    baseline = {int(record["pair"]): record for record in records if record["variant"] == "baseline"}
    candidate = {int(record["pair"]): record for record in records if record["variant"] == "candidate"}
    metrics = {}
    for name in ("score", "instructions_retired", "maximum_rss_bytes", "elapsed_seconds"):
        baseline_values = {pair: record[name] for pair, record in baseline.items()}
        candidate_values = {pair: record[name] for pair, record in candidate.items()}
        paired_changes = [
            (float(candidate_values[pair]) / float(baseline_values[pair]) - 1.0) * 100.0
            for pair in sorted(baseline_values.keys() & candidate_values.keys())
        ]
        baseline_median = statistics.median(baseline_values.values())
        candidate_median = statistics.median(candidate_values.values())
        better = (lambda change: change > 0) if name == "score" else (lambda change: change < 0)
        metrics[name] = {
            "baseline_median": baseline_median,
            "candidate_median": candidate_median,
            "median_absolute_delta": candidate_median - baseline_median,
            "median_change_percent": (candidate_median / baseline_median - 1.0) * 100.0,
            "paired_median_change_percent": statistics.median(paired_changes),
            "candidate_better_pairs": sum(better(change) for change in paired_changes),
            "pairs": len(paired_changes),
        }
    return {
        "metrics": metrics,
        "stdout_equal_every_pair": all(
            semantic_output(baseline[pair]["stdout"])
            == semantic_output(candidate[pair]["stdout"])
            for pair in baseline.keys() & candidate.keys()
        ),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=11)
    parser.add_argument(
        "--out",
        type=Path,
        default=EVIDENCE / "task61-lazy-host-string-splay-fixture-2026-10-09.json",
    )
    args = parser.parse_args()
    for path in (BASELINE, CANDIDATE, RUNNER_SOURCE):
        if not path.is_file():
            raise SystemExit(f"missing input: {path}")
    runner_match = RUNNER_PATTERN.search(RUNNER_SOURCE.read_text())
    if runner_match is None:
        raise SystemExit("could not read quench-bench RUNNER source")

    materialized = TARGET / "materialized-splay.js"
    materialized.write_bytes(
        (SUITE / "base.js").read_bytes()
        + b"\n"
        + (SUITE / "splay.js").read_bytes()
        + b"\n"
        + runner_match.group(1).encode()
    )
    binaries = {"baseline": BASELINE, "candidate": CANDIDATE}
    records = []
    for pair in range(1, args.rounds + 1):
        order = ("baseline", "candidate") if pair % 2 else ("candidate", "baseline")
        outputs = {}
        for variant in order:
            command = ["/usr/bin/time", "-l", str(binaries[variant]), str(materialized)]
            started = time.monotonic()
            result = subprocess.run(
                command, cwd=ROOT, capture_output=True, text=True, check=False
            )
            combined = result.stderr + result.stdout
            score = SCORE.search(result.stdout)
            if result.returncode != 0 or score is None:
                raise RuntimeError(f"{variant}/{pair} failed:\n{combined}")
            outputs[variant] = result.stdout
            records.append(
                {
                    "pair": pair,
                    "order": list(order),
                    "variant": variant,
                    "command": command,
                    "exit_code": result.returncode,
                    "score": float(score.group(1)),
                    "instructions_retired": metric(
                        INSTRUCTIONS, combined, "instructions_retired"
                    ),
                    "maximum_rss_bytes": metric(RSS, combined, "maximum_rss_bytes"),
                    "elapsed_seconds": metric(ELAPSED, combined, "elapsed_seconds"),
                    "wall_seconds": time.monotonic() - started,
                    "stdout": result.stdout,
                    "stderr": result.stderr,
                }
            )
        if semantic_output(outputs["baseline"]) != semantic_output(outputs["candidate"]):
            raise RuntimeError(f"observable output differs in pair {pair}")

    report = {
        "task": 61,
        "experiment": "lazy derived UTF-8 view Splay fixture check",
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "processor": platform.processor(),
            "python": platform.python_version(),
        },
        "rounds": args.rounds,
        "binary_sha256": {name: sha256(path) for name, path in binaries.items()},
        "materialized_sha256": sha256(materialized),
        "runner_source_sha256": sha256(RUNNER_SOURCE),
        "base_source_sha256": sha256(SUITE / "base.js"),
        "fixture_source_sha256": sha256(SUITE / "splay.js"),
        "summary": summarize(records),
        "records": records,
    }
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(args.out)
    print(f"records={len(records)}")


if __name__ == "__main__":
    main()
