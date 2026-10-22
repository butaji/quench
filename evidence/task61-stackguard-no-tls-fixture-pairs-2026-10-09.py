#!/usr/bin/env python3
"""Matched Quench-only fixture pairs for the VM-cached StackGuard candidate."""

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
SUITE = ROOT / "quench-bench/js-engine-benchmark/v8-v7"
RUNNER_SOURCE = ROOT / "quench-bench/src/main.rs"
RUNNER_PATTERN = re.compile(r'const RUNNER: &str = r#"(.*?)"#;', re.DOTALL)
SCORE = re.compile(r"(?m)^Score: ([0-9]+(?:\.[0-9]+)?)\s*$")
INSTRUCTIONS = re.compile(r"(?m)^\s*(\d+)\s+instructions retired\s*$")
CYCLES = re.compile(r"(?m)^\s*(\d+)\s+cycles elapsed\s*$")
RSS = re.compile(r"(?m)^\s*(\d+)\s+maximum resident set size\s*$")
ELAPSED = re.compile(r"(?m)^\s*([0-9.]+)\s+real\b")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def metric(pattern: re.Pattern[str], report: str, name: str) -> int | float:
    match = pattern.search(report)
    if match is None:
        raise RuntimeError(f"time -l did not report {name}: {report}")
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


def run(binary: Path, script: Path, fixture: str, pair: int, variant: str) -> dict:
    command = ["/usr/bin/time", "-l", str(binary), str(script)]
    started = time.monotonic()
    completed = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    report = completed.stderr + completed.stdout
    score = SCORE.search(completed.stdout)
    if completed.returncode != 0 or score is None:
        raise RuntimeError(f"{fixture}/{pair}/{variant} failed:\n{report}")
    return {
        "fixture": fixture,
        "pair": pair,
        "variant": variant,
        "command": command,
        "exit_code": completed.returncode,
        "score": float(score.group(1)),
        "instructions_retired": metric(INSTRUCTIONS, report, "instructions_retired"),
        "cycles_elapsed": metric(CYCLES, report, "cycles_elapsed"),
        "maximum_rss_bytes": metric(RSS, report, "maximum_rss_bytes"),
        "elapsed_seconds": metric(ELAPSED, report, "elapsed_seconds"),
        "wall_seconds": time.monotonic() - started,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--fixtures", nargs="+", required=True)
    parser.add_argument("--rounds", type=int, default=11)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()

    runner_match = RUNNER_PATTERN.search(RUNNER_SOURCE.read_text())
    if runner_match is None:
        raise SystemExit("could not read quench-bench RUNNER source")
    runner = runner_match.group(1).encode()
    base_path = SUITE / "base.js"
    base = base_path.read_bytes()
    out_path = args.out if args.out.is_absolute() else ROOT / args.out
    work = out_path.with_suffix("")
    work.mkdir(parents=True, exist_ok=True)
    inputs = {}
    records = []
    binaries = {"baseline": args.baseline, "candidate": args.candidate}

    for fixture_name in args.fixtures:
        fixture_path = SUITE / fixture_name
        fixture = fixture_path.read_bytes()
        script = work / f"materialized-{fixture_name}"
        script.write_bytes(base + b"\n" + fixture + b"\n" + runner)
        inputs[fixture_name] = {
            "source_sha256": sha256(fixture_path),
            "materialized_sha256": sha256(script),
            "materialized_path": str(script.relative_to(ROOT)),
        }
        for pair in range(1, args.rounds + 1):
            order = ("candidate", "baseline") if pair % 2 else ("baseline", "candidate")
            results = {
                variant: run(binaries[variant], script, fixture_name, pair, variant)
                for variant in order
            }
            if semantic_output(results["baseline"]["stdout"]) != semantic_output(
                results["candidate"]["stdout"]
            ):
                raise RuntimeError(f"observable fixture output differs: {fixture_name}/{pair}")
            records.append({"fixture": fixture_name, "pair": pair, "order": order, "results": results})

    out_path.write_text(
        json.dumps(
            {
                "task": 61,
                "experiment": "VM-cached no-TLS StackGuard fixture gate",
                "platform": platform.platform(),
                "machine": platform.machine(),
                "rounds_per_fixture": args.rounds,
                "binary_sha256": {name: sha256(path) for name, path in binaries.items()},
                "runner_sha256": sha256(RUNNER_SOURCE),
                "base_sha256": sha256(base_path),
                "inputs": inputs,
                "records": records,
            },
            indent=2,
        )
        + "\n"
    )
    print(out_path.relative_to(ROOT))
    print(f"records={len(records)}")


if __name__ == "__main__":
    main()
