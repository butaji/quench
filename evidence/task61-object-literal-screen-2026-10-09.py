#!/usr/bin/env python3
"""Paired production measurements for object literal creation and controls."""

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OUT_DIR = ROOT / "tasks/evidence/task61-object-literal-screen-2026-10-09"
BASELINE = ROOT / "target/pinned/2854be3031531d0e71cbdd4098ae8bdfb64079aff34831a6e9c28bce4113f7b7/quench-node"
CANDIDATE = ROOT / "target/pinned/697a169197b8a6218cfba410bb3fd16a72f3948b846d53c0c3a7d31193be3c02/quench-node"
PER_OP_INPUTS = ROOT / "tasks/evidence/task61-per-op-budget-2026-10-09"
CASES = {
    "object-literal-3": (
        PER_OP_INPUTS / "object-literal-active.cjs",
        PER_OP_INPUTS / "object-literal-zero.cjs",
        True,
    ),
    "object-literal-2": (
        OUT_DIR / "object-literal-2-active.cjs",
        OUT_DIR / "object-literal-2-zero.cjs",
        False,
    ),
    "object-literal-nested-array": (
        OUT_DIR / "object-literal-nested-array-active.cjs",
        OUT_DIR / "object-literal-nested-array-zero.cjs",
        False,
    ),
    "local-store": (
        PER_OP_INPUTS / "local-store-active.cjs",
        PER_OP_INPUTS / "local-store-zero.cjs",
        False,
    ),
}
METRICS = {
    "instructions_retired": re.compile(r"(?m)^\s*(\d+)\s+instructions retired\s*$"),
    "cycles_elapsed": re.compile(r"(?m)^\s*(\d+)\s+cycles elapsed\s*$"),
    "maximum_rss_bytes": re.compile(r"(?m)^\s*(\d+)\s+maximum resident set size\s*$"),
    "elapsed_seconds": re.compile(r"(?m)^\s*([0-9.]+)\s+real\b"),
}


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run(binary, script):
    command = ["/usr/bin/time", "-l", str(binary), str(script)]
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, capture_output=True)
    report = result.stderr.decode(errors="replace") + result.stdout.decode(errors="replace")
    metrics = {}
    for name, pattern in METRICS.items():
        match = pattern.search(report)
        value = match.group(1) if match else None
        metrics[name] = (
            float(value) if name == "elapsed_seconds" else int(value)
        ) if value is not None else None
    return {
        "command": command,
        "exit_code": result.returncode,
        "metrics": metrics,
        "stdout_sha256": hashlib.sha256(result.stdout).hexdigest(),
        "stderr": result.stderr.decode(errors="replace"),
        "wall_seconds": time.monotonic() - started,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--pairs", type=int, default=3)
    parser.add_argument("--output", default="screen.json")
    arguments = parser.parse_args()
    if arguments.pairs < 1:
        raise SystemExit("pairs must be positive")
    binaries = {"baseline": BASELINE, "candidate": CANDIDATE}
    records = []
    for case_index, (case, (active_script, zero_script, affected)) in enumerate(CASES.items()):
        iterations_match = re.search(r"var N = (\d+);", active_script.read_text())
        if iterations_match is None:
            raise SystemExit(f"missing iteration count in {active_script}")
        iterations = int(iterations_match.group(1))
        for pair in range(1, arguments.pairs + 1):
            order = list(binaries)
            if (case_index + pair) % 2:
                order.reverse()
            engines = {}
            for name in order:
                binary = binaries[name]
                engines[name] = {
                    "active": run(binary, active_script),
                    "zero_iteration": run(binary, zero_script),
                }
                active = engines[name]["active"]["metrics"]
                zero = engines[name]["zero_iteration"]["metrics"]
                engines[name]["net_per_iteration"] = {
                    metric: (
                        (active[metric] - zero[metric]) / iterations
                        if active[metric] is not None and zero[metric] is not None
                        else None
                    )
                    for metric in ("instructions_retired", "cycles_elapsed", "elapsed_seconds")
                }
            records.append({
                "case": case,
                "targeted_by_candidate": affected,
                "pair": pair,
                "iterations": iterations,
                "order": order,
                "engines": engines,
            })
            out = OUT_DIR / f"{Path(arguments.output).stem}.partial.json"
            out.write_text(json.dumps({"pairs": records}, indent=2) + "\n")

    report = {
        "schema": "quench.task61.object-literal-measurement.v1",
        "date": "2026-10-09",
        "platform": platform.platform(),
        "machine": platform.machine(),
        "cpu": "Apple M4",
        "profile": "production-thin-lto; codegen-units=1; panic=abort",
        "baseline_source_revision": "855a083a4e398743b359a8ac1efeecee4eea60d4",
        "candidate_source_revision": "b2a6b083429f90a87c321aef317de2e44fe20419 + dirty object-literal diff",
        "candidate_measured_sha256": digest(CANDIDATE),
        "baseline_measured_sha256": digest(BASELINE),
        "binaries": {
            name: {"path": str(path), "sha256": digest(path)}
            for name, path in binaries.items()
        },
        "inputs": {
            str(path.relative_to(ROOT)): digest(path)
            for scripts in CASES.values()
            for path in scripts[:2]
        },
        "method": (
            f"{arguments.pairs} alternating Quench-only production pairs per case. For each binary, "
            "subtract the zero-iteration process counters from active counters and divide "
            "by N. Cycles are the decision metric; local-store, two-key, and nested-array "
            "rows are unaffected controls."
        ),
        "pairs": records,
        "free_disk_bytes_at_start": shutil.disk_usage(ROOT).free,
        "test_or_timing_campaigns_active": False,
    }
    output = OUT_DIR / arguments.output
    output.write_text(json.dumps(report, indent=2) + "\n")
    (OUT_DIR / f"{Path(arguments.output).stem}.partial.json").unlink(missing_ok=True)
    print(output.relative_to(ROOT))


if __name__ == "__main__":
    main()
