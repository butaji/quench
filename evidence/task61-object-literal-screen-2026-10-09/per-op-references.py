#!/usr/bin/env python3
"""Three-repetition no-JIT per-op screen for two-key object literal shapes."""

import hashlib
import json
import os
import platform
import re
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent / "per-op-references.json"
INPUT = Path(__file__).resolve().parent
OLD = ROOT / "tasks/evidence/task61-per-op-budget-2026-10-09.json"
CANDIDATE = ROOT / "target/pinned/697a169197b8a6218cfba410bb3fd16a72f3948b846d53c0c3a7d31193be3c02/quench-node"
CASES = {
    "object-literal-2": (INPUT / "object-literal-2-active.cjs", INPUT / "object-literal-2-zero.cjs"),
    "object-literal-nested-array": (INPUT / "object-literal-nested-array-active.cjs", INPUT / "object-literal-nested-array-zero.cjs"),
}
PATTERNS = {
    "instructions_retired": re.compile(r"(?m)^\s*(\d+)\s+instructions retired\s*$"),
    "cycles_elapsed": re.compile(r"(?m)^\s*(\d+)\s+cycles elapsed\s*$"),
    "maximum_rss_bytes": re.compile(r"(?m)^\s*(\d+)\s+maximum resident set size\s*$"),
}


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def measure(engine, script):
    command = ["/usr/bin/time", "-l", engine["binary"], *engine["args"], str(script)]
    env = {**os.environ, **engine["environment"]}
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True)
    report = result.stderr + result.stdout
    metrics = {}
    for name, pattern in PATTERNS.items():
        match = pattern.search(report)
        metrics[name] = int(match.group(1)) if match else None
    return {
        "command": command,
        "exit_code": result.returncode,
        "metrics": metrics,
        "stdout_sha256": hashlib.sha256(result.stdout.encode()).hexdigest(),
        "stderr": result.stderr,
        "wall_seconds": time.monotonic() - started,
    }


def main():
    source = json.loads(OLD.read_text())
    engines = source["engines"]
    engines["quench"] = {
        "binary": str(CANDIDATE),
        "sha256": digest(CANDIDATE),
        "args": [],
        "environment": {},
    }
    records = []
    for case_index, (case, (active, zero)) in enumerate(CASES.items()):
        iterations = int(re.search(r"var N = (\d+);", active.read_text()).group(1))
        names = list(engines)
        for repetition in range(1, 4):
            order = names[repetition % len(names):] + names[:repetition % len(names)]
            for name in order:
                engine = engines[name]
                active_run = measure(engine, active)
                zero_run = measure(engine, zero)
                if active_run["exit_code"] or zero_run["exit_code"]:
                    raise RuntimeError(f"{case}/{name} failed: {active_run} {zero_run}")
                records.append({
                    "case": case,
                    "engine": name,
                    "repetition": repetition,
                    "iterations": iterations,
                    "active": active_run,
                    "zero_iteration": zero_run,
                    "net_per_iteration": {
                        metric: (active_run["metrics"][metric] - zero_run["metrics"][metric]) / iterations
                        for metric in ("instructions_retired", "cycles_elapsed")
                    },
                })
    summary = {}
    for case in CASES:
        rows = [record for record in records if record["case"] == case]
        summary[case] = {}
        for engine_name in engines:
            engine_rows = [row for row in rows if row["engine"] == engine_name]
            summary[case][engine_name] = {
                metric: sorted(row["net_per_iteration"][metric] for row in engine_rows)[1]
                for metric in ("instructions_retired", "cycles_elapsed")
            }
    report = {
        "schema": "quench.task61.object-literal-reference-screen.v1",
        "date": "2026-10-09",
        "host": {"platform": platform.platform(), "machine": platform.machine(), "cpu": "Apple M4"},
        "profile": "production-thin-lto; codegen-units=1; panic=abort for Quench",
        "repetitions": 3,
        "baseline_engine_metadata": OLD.relative_to(ROOT).as_posix(),
        "candidate_sha256": digest(CANDIDATE),
        "inputs": {str(path.relative_to(ROOT)): digest(path) for pair in CASES.values() for path in pair},
        "summary_median_per_iteration": summary,
        "records": records,
    }
    OUT.write_text(json.dumps(report, indent=2) + "\n")
    print(OUT.relative_to(ROOT))
    for case, values in summary.items():
        print(case, values)


if __name__ == "__main__":
    main()
