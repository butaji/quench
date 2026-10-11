#!/usr/bin/env python3
"""Eleven alternating Quench-only pairs plus an unaffected local-store control."""

import hashlib
import json
import platform
import re
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
INPUTS = ROOT / "tasks/evidence/task61-per-op-budget-2026-10-09"
BASELINE = ROOT / "target/pinned/2854be3031531d0e71cbdd4098ae8bdfb64079aff34831a6e9c28bce4113f7b7/quench-node"
CANDIDATE = ROOT / "target/candidate-b/production/quench-node"
OUT = ROOT / "tasks/evidence/task61-regexp-inline-ascii-gate-2026-10-09.json"
CASES = ("regexp-exec", "regexp-replace", "local-store")
PAIRS = 11
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
    records = []
    binaries = {"baseline": BASELINE, "candidate": CANDIDATE}
    for case_index, case in enumerate(CASES):
        active_script = INPUTS / f"{case}-active.cjs"
        zero_script = INPUTS / f"{case}-zero.cjs"
        iterations = int(re.search(r"var N = (\d+);", active_script.read_text()).group(1))
        for pair in range(1, PAIRS + 1):
            names = list(binaries)
            if (case_index + pair) % 2:
                names.reverse()
            paired = {}
            for name in names:
                binary = binaries[name]
                paired[name] = {
                    "active": run(binary, active_script),
                    "zero_iteration": run(binary, zero_script),
                }
            for result in paired.values():
                active = result["active"]["metrics"]
                zero = result["zero_iteration"]["metrics"]
                result["net_per_iteration"] = {}
                for metric in ("instructions_retired", "cycles_elapsed", "elapsed_seconds"):
                    result["net_per_iteration"][metric] = (
                        (active[metric] - zero[metric]) / iterations
                        if active[metric] is not None and zero[metric] is not None
                        else None
                    )
            records.append({
                "case": case,
                "pair": pair,
                "iterations": iterations,
                "order": names,
                "engines": paired,
            })
            OUT.with_suffix(".partial.json").write_text(
                json.dumps({"records": records}, indent=2) + "\n"
            )

    report = {
        "schema": "quench.task61.regexp-inline-ascii-gate.v1",
        "date": "2026-10-09",
        "platform": platform.platform(),
        "machine": platform.machine(),
        "profile": "production-thin-lto",
        "baseline_commit": "cbb72f34aa3a8cd15ca8ce8c68f32d9b8650b393",
        "candidate_source_dirty": True,
        "source_sha256": {
            "crates/quench-regexp/Cargo.toml": digest(ROOT / "crates/quench-regexp/Cargo.toml"),
            "crates/quench-regexp/src/lib.rs": digest(ROOT / "crates/quench-regexp/src/lib.rs"),
            "Cargo.lock": digest(ROOT / "Cargo.lock"),
        },
        "binaries": {
            name: {"path": str(path), "sha256": digest(path)}
            for name, path in binaries.items()
        },
        "inputs": {
            f"{case}-{suffix}.cjs": digest(INPUTS / f"{case}-{suffix}.cjs")
            for case in CASES
            for suffix in ("active", "zero")
        },
        "method": (
            "Eleven alternating production binary pairs per case. Each active and zero-iteration "
            "process is measured with /usr/bin/time -l; subtract process counters and divide by "
            "loop iterations. local-store is the unrelated codegen-drift control."
        ),
        "pairs": records,
    }
    OUT.write_text(json.dumps(report, indent=2) + "\n")
    OUT.with_suffix(".partial.json").unlink(missing_ok=True)
    print(OUT.relative_to(ROOT))


if __name__ == "__main__":
    main()
