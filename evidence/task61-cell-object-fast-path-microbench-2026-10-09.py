#!/usr/bin/env python3
"""Three alternating M4 screens for the Cell Object/Array fast path."""
import argparse
import hashlib
import json
import platform
import re
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DIR = ROOT / "tasks/evidence/task61-cell-object-fast-path-microbench-2026-10-09"
PAIRS = 3
METRICS = {
    "instructions_retired": re.compile(r"(?m)^\s*(\d+)\s+instructions retired\s*$"),
    "cycles_elapsed": re.compile(r"(?m)^\s*(\d+)\s+cycles elapsed\s*$"),
    "maximum_rss_bytes": re.compile(r"(?m)^\s*(\d+)\s+maximum resident set size\s*$"),
    "elapsed_seconds": re.compile(r"(?m)^\s*([0-9.]+)\s+real\b"),
}
CASES = ("read", "write", "array-read", "array-write", "local-control")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(binary, script):
    start = time.monotonic()
    result = subprocess.run(["/usr/bin/time", "-l", str(binary), str(script)], cwd=ROOT, capture_output=True, text=True)
    report = result.stderr + result.stdout
    if result.returncode:
        raise RuntimeError(f"{binary} {script} failed:\n{report}")
    values = {}
    for key, regex in METRICS.items():
        match = regex.search(report)
        if not match:
            raise RuntimeError(f"missing {key} in {report}")
        val = match.group(1)
        values[key] = float(val) if key == "elapsed_seconds" else int(val)
    values["wall_seconds"] = time.monotonic() - start
    values["stdout"] = result.stdout
    return values


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    records = []
    for case in CASES:
        for pair in range(1, PAIRS + 1):
            order = ("candidate", "baseline") if pair % 2 else ("baseline", "candidate")
            results = {}
            for variant in order:
                active = run(binaries[variant], DIR / f"{case}.cjs")
                empty = run(binaries[variant], DIR / f"{case}-zero.cjs")
                results[variant] = {
                    "active": active,
                    "zero_iteration": empty,
                    "net": {m: active[m] - empty[m] for m in METRICS if m != "maximum_rss_bytes"},
                }
            records.append({"case": case, "pair": pair, "order": order, "results": results})
    output = args.out if args.out.is_absolute() else ROOT / args.out
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({
        "experiment": "Cell::Object/Array early-return three-pair screen",
        "platform": platform.platform(), "machine": platform.machine(), "pairs_per_case": PAIRS,
        "iteration_count": 2_000_000,
        "binary_sha256": {name: sha256(path) for name, path in binaries.items()},
        "inputs": {case: sha256(DIR / f"{case}.cjs") for case in CASES},
        "zero_iteration_inputs": {case: sha256(DIR / f"{case}-zero.cjs") for case in CASES},
        "records": records,
    }, indent=2) + "\n")
    print(output.relative_to(ROOT))
    print(f"records={len(records)}")


if __name__ == "__main__":
    main()
