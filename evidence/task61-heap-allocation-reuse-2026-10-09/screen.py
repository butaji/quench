#!/usr/bin/env python3
"""Paired allocation measurements for sparse-side-table slot reuse."""

import argparse
import hashlib
import json
import platform
import random
import re
import shutil
import statistics
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
OUT_DIR = Path(__file__).resolve().parent
CASES = {
    "sparse-side-table": (
        OUT_DIR / "sparse-side-table-active.cjs",
        OUT_DIR / "sparse-side-table-zero.cjs",
    ),
    "allocation-control-no-large-array": (
        OUT_DIR / "no-side-table-active.cjs",
        OUT_DIR / "no-side-table-zero.cjs",
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


def measure(binary, source):
    command = ["/usr/bin/time", "-l", str(binary), str(source)]
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


def relative_improvement(baseline, candidate):
    if baseline == 0:
        return None
    return 1.0 - candidate / baseline


def bootstrap_interval(values, seed):
    randomizer = random.Random(seed)
    estimates = [
        statistics.median(randomizer.choices(values, k=len(values)))
        for _ in range(20000)
    ]
    estimates.sort()
    return [estimates[499], estimates[19499]]


def parse_arguments():
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--baseline-revision", required=True)
    parser.add_argument("--candidate-revision", required=True)
    parser.add_argument("--pairs", type=int, default=3)
    parser.add_argument("--output", default="screen.json")
    arguments = parser.parse_args()
    if arguments.pairs < 1:
        raise SystemExit("pairs must be positive")
    return arguments


def measured_variant(binary, active, zero, iterations, case, name):
    active_run = measure(binary, active)
    zero_run = measure(binary, zero)
    if active_run["exit_code"] or zero_run["exit_code"]:
        raise RuntimeError(f"{case}/{name} failed: {active_run} {zero_run}")
    if active_run["stdout_sha256"] != zero_run["stdout_sha256"]:
        raise RuntimeError(f"{case}/{name} changed output by iteration count")
    return {
        "active": active_run,
        "zero_iteration": zero_run,
        "net_per_iteration": {
            metric: (active_run["metrics"][metric] - zero_run["metrics"][metric])
            / iterations
            for metric in ("instructions_retired", "cycles_elapsed")
        },
    }


def measured_case(case, inputs, pair_count, binaries, case_index):
    active, zero = inputs
    match = re.search(r"var N = (\d+);", active.read_text())
    if match is None:
        raise SystemExit(f"missing iteration count in {active}")
    iterations = int(match.group(1))
    records = []
    for pair in range(1, pair_count + 1):
        order = list(binaries)
        if (case_index + pair) % 2:
            order.reverse()
        variants = {
            name: measured_variant(binaries[name], active, zero, iterations, case, name)
            for name in order
        }
        if variants["baseline"]["active"]["stdout_sha256"] != variants["candidate"]["active"]["stdout_sha256"]:
            raise RuntimeError(f"{case} baseline/candidate output mismatch")
        records.append({
            "case": case,
            "pair": pair,
            "iterations": iterations,
            "order": order,
            "variants": variants,
        })
    return records


def summarize_case(rows):
    summary = {}
    for metric in ("instructions_retired", "cycles_elapsed"):
        improvements = [
            relative_improvement(
                row["variants"]["baseline"]["net_per_iteration"][metric],
                row["variants"]["candidate"]["net_per_iteration"][metric],
            )
            for row in rows
        ]
        summary[metric] = {
            "pair_improvements": improvements,
            "median_relative_improvement": statistics.median(improvements),
            "bootstrap_95_percent_interval": bootstrap_interval(improvements, seed=6102026),
        }
    baseline_rss = [
        row["variants"]["baseline"]["active"]["metrics"]["maximum_rss_bytes"]
        for row in rows
    ]
    candidate_rss = [
        row["variants"]["candidate"]["active"]["metrics"]["maximum_rss_bytes"]
        for row in rows
    ]
    rss = [candidate - baseline for baseline, candidate in zip(baseline_rss, candidate_rss)]
    summary["maximum_rss_bytes"] = {
        "pair_changes": rss,
        "median_change": statistics.median(rss),
        "baseline_median": statistics.median(baseline_rss),
        "candidate_median": statistics.median(candidate_rss),
        "relative_change_of_medians": (
            statistics.median(candidate_rss) / statistics.median(baseline_rss) - 1
        ),
    }
    return summary


def measurement_report(args, binaries, records):
    summary = {
        case: summarize_case([row for row in records if row["case"] == case])
        for case in CASES
    }
    return {
        "schema": "quench.task61.heap-allocation-reuse.v1",
        "date": "2026-10-09",
        "platform": platform.platform(),
        "machine": platform.machine(),
        "cpu": "Apple M4",
        "physical_memory_bytes": 17179869184,
        "free_disk_bytes_when_reported": shutil.disk_usage(ROOT).free,
        "profile": "production-thin-lto; codegen-units=1; panic=abort",
        "baseline_source_revision": args.baseline_revision,
        "candidate_source_revision": args.candidate_revision,
        "binaries": {
            name: {"path": str(path), "sha256": digest(path)}
            for name, path in binaries.items()
        },
        "inputs": {
            str(path.relative_to(ROOT)): digest(path)
            for pair in CASES.values()
            for path in pair
        },
        "method": (
            f"{args.pairs} alternating Quench-only production pairs per case; subtract each "
            "binary's zero-iteration counters and divide by N. Cycles are the decision metric; "
            "the allocation control omits explicit large sparse-array setup. RSS uses active-run maximum."
        ),
        "control_interpretation": (
            "The control does not prove the VM's sparse-array side table is absent; Node bootstrap "
            "could have initialized it before the script runs."
        ),
        "summary": summary,
        "pairs": records,
    }


def main():
    args = parse_arguments()
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    output = OUT_DIR / args.output
    partial = OUT_DIR / f"{output.stem}.partial.json"
    records = []
    for index, (case, inputs) in enumerate(CASES.items()):
        records.extend(measured_case(case, inputs, args.pairs, binaries, index))
        partial.write_text(json.dumps({"pairs": records}, indent=2) + "\n")
    report = measurement_report(args, binaries, records)
    output.write_text(json.dumps(report, indent=2) + "\n")
    partial.unlink(missing_ok=True)
    print(output.relative_to(ROOT))


if __name__ == "__main__":
    main()
