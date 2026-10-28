#!/usr/bin/env python3
"""Current CJS-shaped per-op budget across Quench, rqj, qjs, Node jitless and Bun no-JIT."""
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
INPUTS = ROOT / "tasks/evidence/task61-per-op-budget-2026-10-09"
METRICS = {
    "instructions_retired": re.compile(r"(?m)^\s*(\d+)\s+instructions retired\s*$"),
    "cycles_elapsed": re.compile(r"(?m)^\s*(\d+)\s+cycles elapsed\s*$"),
    "maximum_rss_bytes": re.compile(r"(?m)^\s*(\d+)\s+maximum resident set size\s*$"),
    "elapsed_seconds": re.compile(r"(?m)^\s*([0-9.]+)\s+real\b"),
}
CASES = (
    "local-store", "field-read", "array-read", "array-write", "compound-index",
    "local-call", "construct", "closure-create", "instanceof", "typeof",
    "char-code-at", "substring", "math-floor", "object-literal", "array-literal",
    "regexp-exec", "regexp-replace", "from-char-code",
)


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run(engine, binary, args, script, env, label):
    command = ["/usr/bin/time", "-l", str(binary), *args, str(script)]
    process_env = os.environ.copy()
    process_env.update(env)
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env=process_env, capture_output=True, text=True)
    report = result.stderr + result.stdout
    metrics = {}
    for name, pattern in METRICS.items():
        match = pattern.search(report)
        if not match:
            metrics[name] = None
        else:
            value = match.group(1)
            metrics[name] = float(value) if name == "elapsed_seconds" else int(value)
    return {
        "command": command,
        "exit_code": result.returncode,
        "metrics": metrics,
        "stdout": result.stdout,
        "stderr": result.stderr,
        "failure": report if result.returncode else None,
        "wall_seconds": time.monotonic() - started,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--quench", required=True, type=Path)
    parser.add_argument("--rqj", required=True, type=Path)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    engines = {
        "quench": (args.quench.resolve(), [], {}),
        "rqj": (args.rqj.resolve(), [], {}),
        "qjs": (Path(shutil.which("qjs")), [], {}),
        "node_jitless": (Path(shutil.which("node")), ["--jitless"], {}),
        "bun_no_jit": (Path(shutil.which("bun")), [], {"BUN_JSC_useJIT": "0"}),
    }
    if any(binary is None for binary, _, _ in engines.values()):
        raise SystemExit("missing qjs/node/bun from PATH")
    hashes = {name: sha256(binary) for name, (binary, _, _) in engines.items()}
    records = []
    partial_path = args.out.with_suffix(".partial.json")
    out_path = args.out if args.out.is_absolute() else ROOT / args.out
    partial_path = out_path.with_suffix(".partial.json")
    for case_index, case in enumerate(CASES):
        active_script = INPUTS / f"{case}-active.cjs"
        zero_script = INPUTS / f"{case}-zero.cjs"
        iterations = int(re.search(r"var N = (\d+);", active_script.read_text()).group(1))
        engine_names = list(engines)
        for repetition in range(1, args.repetitions + 1):
            shift = (case_index + repetition - 1) % len(engine_names)
            order = engine_names[shift:] + engine_names[:shift]
            for engine_name in order:
                binary, engine_args, env = engines[engine_name]
                active = run(engine_name, binary, engine_args, active_script, env, "active")
                zero = run(engine_name, binary, engine_args, zero_script, env, "zero")
                net = {}
                per_iteration = {}
                for metric in ("instructions_retired", "cycles_elapsed", "elapsed_seconds"):
                    active_value = active["metrics"][metric]
                    zero_value = zero["metrics"][metric]
                    if active["exit_code"] or zero["exit_code"] or active_value is None or zero_value is None:
                        net[metric] = None
                        per_iteration[metric] = None
                    else:
                        net[metric] = active_value - zero_value
                        per_iteration[metric] = net[metric] / iterations
                records.append({
                    "case": case,
                    "iterations": iterations,
                    "repetition": repetition,
                    "engine": engine_name,
                    "active": active,
                    "zero_iteration": zero,
                    "net": net,
                    "per_iteration": per_iteration,
                })
            partial_path.parent.mkdir(parents=True, exist_ok=True)
            partial_path.write_text(json.dumps({"records": records}, indent=2) + "\n")
            print(f"{case}: repetition {repetition}/{args.repetitions}", flush=True)
    input_hashes = {}
    for case in CASES:
        for suffix in ("active", "zero"):
            path = INPUTS / f"{case}-{suffix}.cjs"
            input_hashes[path.name] = sha256(path)
    report = {
        "schema": "quench.task61.per-op-budget.v1",
        "date": "2026-10-09",
        "platform": platform.platform(),
        "machine": platform.machine(),
        "quench_profile": "production-thin-lto",
        "iterations_subtract_zero_iteration_process": True,
        "repetitions_per_engine_case": args.repetitions,
        "engines": {
            name: {"binary": str(binary), "sha256": hashes[name], "args": engine_args, "environment": env}
            for name, (binary, engine_args, env) in engines.items()
        },
        "inputs": input_hashes,
        "records": records,
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(report, indent=2) + "\n")
    partial_path.unlink(missing_ok=True)
    print(out_path.relative_to(ROOT))
    print(f"records={len(records)}")


if __name__ == "__main__":
    main()
