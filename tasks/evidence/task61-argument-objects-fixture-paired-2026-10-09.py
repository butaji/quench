#!/usr/bin/env python3
"""Matched Quench-only V8-v7 argument-object fixture screen."""

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
TARGET = ROOT / "target/iteration/task61-argument-shapes-profile-20261009"
BASELINE = ROOT / "target/iteration/task61-local-bounds-prod/production/quench-node"
CANDIDATE = TARGET / "production/quench-node"
SUITE = ROOT / "quench-bench/js-engine-benchmark/v8-v7"
FIXTURES = ("raytrace.js", "earley-boyer.js")
RUNNER_SOURCE = ROOT / "quench-bench/src/main.rs"
RUNNER_PATTERN = re.compile(r'const RUNNER: &str = r#"(.*?)"#;', re.DOTALL)
SCORE = re.compile(r"(?m)^Score: ([0-9]+(?:\.[0-9]+)?)\s*$")
INSTRUCTIONS = re.compile(r"^\s*(\d+)\s+instructions retired\s*$", re.MULTILINE)
CYCLES = re.compile(r"^\s*(\d+)\s+cycles elapsed\s*$", re.MULTILINE)
RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size\s*$", re.MULTILINE)
ELAPSED = re.compile(r"^\s*([0-9.]+)\s+real\b", re.MULTILINE)


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def metric(pattern: re.Pattern[str], text: str, name: str) -> int | float:
    match = pattern.search(text)
    if match is None:
        raise RuntimeError(f"time -l did not report {name}: {text}")
    value = match.group(1)
    return float(value) if name == "elapsed_seconds" else int(value)


def materialize(base: bytes, fixture: bytes, runner: bytes, path: Path) -> None:
    path.write_bytes(base + b"\n" + fixture + b"\n" + runner)


def run(binary: Path, script: Path, fixture: str, pair: int, variant: str) -> dict:
    command = ["/usr/bin/time", "-l", str(binary), str(script)]
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=False)
    combined = result.stderr + result.stdout
    score = SCORE.search(result.stdout)
    if result.returncode != 0 or score is None:
        raise RuntimeError(f"{fixture}/{pair}/{variant} failed:\n{combined}")
    return {
        "fixture": fixture,
        "pair": pair,
        "variant": variant,
        "command": command,
        "exit_code": result.returncode,
        "score": float(score.group(1)),
        "instructions_retired": metric(INSTRUCTIONS, combined, "instructions_retired"),
        "cycles_elapsed": metric(CYCLES, combined, "cycles_elapsed"),
        "maximum_rss_bytes": metric(RSS, combined, "maximum_rss_bytes"),
        "elapsed_seconds": metric(ELAPSED, combined, "elapsed_seconds"),
        "wall_seconds": time.monotonic() - started,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }


def semantic_output(stdout: str) -> str:
    return "\n".join(
        line
        for line in stdout.splitlines()
        if not line.startswith("Score: ")
        and line != "----"
        and not line.startswith("__quenchBenchResult: ")
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=11)
    parser.add_argument(
        "--out",
        type=Path,
        default=EVIDENCE / "task61-argument-objects-fixture-paired-2026-10-09.json",
    )
    args = parser.parse_args()

    for path in (BASELINE, CANDIDATE, RUNNER_SOURCE):
        if not path.is_file():
            raise SystemExit(f"missing input: {path}")
    runner_match = RUNNER_PATTERN.search(RUNNER_SOURCE.read_text())
    if runner_match is None:
        raise SystemExit("could not read quench-bench RUNNER source")
    runner = runner_match.group(1).encode()
    base = (SUITE / "base.js").read_bytes()
    TARGET.mkdir(parents=True, exist_ok=True)

    inputs = {}
    records = []
    binaries = {"baseline": BASELINE, "candidate": CANDIDATE}
    for fixture_name in FIXTURES:
        fixture_path = SUITE / fixture_name
        fixture_bytes = fixture_path.read_bytes()
        materialized = TARGET / f"materialized-{fixture_name}"
        materialize(base, fixture_bytes, runner, materialized)
        inputs[fixture_name] = {
            "source_path": str(fixture_path.relative_to(ROOT)),
            "source_sha256": sha256_bytes(fixture_bytes),
            "materialized_sha256": sha256(materialized),
            "materialized_path": str(materialized.relative_to(ROOT)),
        }
        for pair in range(1, args.rounds + 1):
            order = ("candidate", "baseline") if pair % 2 else ("baseline", "candidate")
            results = {
                variant: run(binaries[variant], materialized, fixture_name, pair, variant)
                for variant in order
            }
            baseline_output = semantic_output(results["baseline"]["stdout"])
            candidate_output = semantic_output(results["candidate"]["stdout"])
            if baseline_output != candidate_output:
                raise RuntimeError(f"observable fixture output differs: {fixture_name}/{pair}")
            records.append({"fixture": fixture_name, "pair": pair, "order": order, "results": results})

    report = {
        "task": 61,
        "experiment": "arguments-object template V8-v7 fixture check",
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "processor": platform.processor(),
            "python": platform.python_version(),
        },
        "rounds_per_fixture": args.rounds,
        "binary_sha256": {name: sha256(path) for name, path in binaries.items()},
        "runner_sha256": sha256(RUNNER_SOURCE),
        "base_sha256": sha256(SUITE / "base.js"),
        "inputs": inputs,
        "records": records,
    }
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(args.out)
    print(f"records={len(records)}")


if __name__ == "__main__":
    main()
