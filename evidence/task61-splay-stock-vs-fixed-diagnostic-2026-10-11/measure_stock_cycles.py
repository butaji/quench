#!/usr/bin/env python3
"""Measure stock-shaped Splay process counters and exact run() calls.

The stock harness is unchanged inside each measured run. A one-shot callback
records the existing loop counter after each one-second Measure interval.
The K=0 twin performs the stock runner's Setup and TearDown without entering
Measure, isolating process/bootstrap and setup costs.
"""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import re
import subprocess
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = pathlib.Path(__file__).resolve().parent
SUITE = ROOT / "quench-bench/js-engine-benchmark/v8-v7"
BASE = SUITE / "base.js"
FIXTURE = SUITE / "splay.js"
PREVIOUS = ROOT / "evidence/task61-splay-refresh-ba3-2026-10-11/stock-harness-splay-11.json"
ROUNDS = 11
MAX_LOAD_PER_LOGICAL_CPU = 1.0

COUNTER_PATTERNS = {
    "instructions": re.compile(r"([0-9,]+)\s+instructions retired"),
    "cycles": re.compile(r"([0-9,]+)\s+cycles elapsed"),
    "peak_rss_bytes": re.compile(r"([0-9,]+)\s+maximum resident set size"),
}
TIME_PATTERN = re.compile(r"([0-9.]+) real\s+([0-9.]+) user\s+([0-9.]+) sys")


def materialize() -> tuple[pathlib.Path, pathlib.Path]:
    base = BASE.read_text()
    needle = """    if (data != null) {
      data.runs += n;
      data.elapsed += elapsed;
    }
"""
    replacement = """    __quenchRecordMeasure(benchmark.name, data == null ? \"warmup\" : \"measured\", n, elapsed);
    if (data != null) {
      data.runs += n;
      data.elapsed += elapsed;
    }
"""
    if base.count(needle) != 1:
        raise RuntimeError("stock Measure accounting block was not unique")
    instrumented_base = base.replace(needle, replacement)
    fixture = FIXTURE.read_text()
    count_runner = r"""
var __quenchMeasureCounts = [];
function __quenchRecordMeasure(benchmark, phase, calls, elapsedMs) {
  __quenchMeasureCounts.push({ benchmark: benchmark, phase: phase, calls: calls, elapsed_ms: elapsedMs });
}
let __quenchBenchSucceeded = true;
const __quenchBenchPrint = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
BenchmarkSuite.RunSuites({
  NotifyResult(name, result) { __quenchBenchPrint("__stock_result:" + name + ":" + result); },
  NotifyError(name, error) { __quenchBenchSucceeded = false; __quenchBenchPrint("__stock_error:" + name + ":" + error); },
  NotifyScore(score) {
    __quenchBenchPrint("__stock_score:" + score);
    __quenchBenchPrint("__stock_measure_counts:" + JSON.stringify(__quenchMeasureCounts));
  },
});
"""
    k0_runner = r"""
BenchmarkSuite.prototype.RunSingleBenchmark = function() { return null; };
BenchmarkSuite.RunSuites({
  NotifyResult() {},
  NotifyError() {},
  NotifyScore() {},
});
"""
    stock = OUT / "stock-instrumented-splay.js"
    k0 = OUT / "stock-k0-splay.js"
    stock.write_text(instrumented_base + "\n" + fixture + "\n" + count_runner)
    k0.write_text(base + "\n" + fixture + "\n" + k0_runner)
    return stock, k0


def host_load() -> list[float]:
    return list(os.getloadavg())


def run_one(engine: dict, source: pathlib.Path, phase: str, round_index: int) -> dict:
    argv = [engine["executable"], *engine.get("argv", []), str(source)]
    environment = engine.get("environment", {}).copy()
    before = host_load()
    logical_cpus = os.cpu_count() or 1
    if before[0] / logical_cpus >= MAX_LOAD_PER_LOGICAL_CPU:
        raise RuntimeError(
            f"timing refused: one-minute load per logical CPU is {before[0] / logical_cpus:.2f}"
        )
    started = time.time_ns()
    process = subprocess.run(
        ["/usr/bin/time", "-l", *argv],
        cwd=ROOT,
        env=environment,
        text=True,
        capture_output=True,
        check=False,
    )
    elapsed_ns = time.time_ns() - started
    after = host_load()
    combined = process.stdout + "\n" + process.stderr
    result = {
        "round": round_index,
        "engine": engine["name"],
        "phase": phase,
        "argv": argv,
        "exit_code": process.returncode,
        "elapsed_ns": elapsed_ns,
        "host_load_before": before,
        "host_load_after": after,
        "logical_cpus": logical_cpus,
        "load_per_logical_cpu_max": max(before[0], after[0]) / logical_cpus,
        "timing_clean": max(before[0], after[0]) / logical_cpus < MAX_LOAD_PER_LOGICAL_CPU,
        "stdout": process.stdout,
        "stderr": process.stderr,
    }
    for name, pattern in COUNTER_PATTERNS.items():
        match = pattern.search(combined)
        result[name] = int(match.group(1).replace(",", "")) if match else None
    time_match = TIME_PATTERN.search(combined)
    result["wall_seconds_reported"] = float(time_match.group(1)) if time_match else None
    result["user_seconds"] = float(time_match.group(2)) if time_match else None
    result["system_seconds"] = float(time_match.group(3)) if time_match else None
    result["cpu_seconds"] = (
        result["user_seconds"] + result["system_seconds"]
        if time_match
        else None
    )
    match = re.search(r"__stock_measure_counts:(\[[^\n]+\])", process.stdout)
    result["measure_counts"] = json.loads(match.group(1)) if match else []
    result["total_run_calls"] = sum(item["calls"] for item in result["measure_counts"])
    return result


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    initial_load_per_cpu = host_load()[0] / (os.cpu_count() or 1)
    if initial_load_per_cpu >= MAX_LOAD_PER_LOGICAL_CPU:
        raise SystemExit(
            f"timing refused: one-minute load per logical CPU is {initial_load_per_cpu:.2f}"
        )
    stock, k0 = materialize()
    previous = json.loads(PREVIOUS.read_text())
    engines = {engine["name"]: engine for engine in previous["engines"]}
    order = ["quench", "node", "bun", "quickjs"]
    results = []
    for round_index in range(ROUNDS):
        rotated = order[round_index:] + order[:round_index]
        for name in rotated:
            engine = engines[name]
            phases = (
                (("stock-work", stock), ("stock-k0", k0))
                if round_index % 2
                else (("stock-k0", k0), ("stock-work", stock))
            )
            for phase, source in phases:
                results.append(run_one(engine, source, phase, round_index))
            report = {
                "schema": 2,
                "host": {"os": os.uname().sysname, "machine": os.uname().machine, "load_average": host_load()},
                "source_revision": previous.get("source_revision"),
                "runtime_source_revision": "ba3eee054",
                "source_metric_origin": str(PREVIOUS.relative_to(ROOT)),
                "stock_source_sha256": hashlib.sha256(stock.read_bytes()).hexdigest(),
                "stock_k0_source_sha256": hashlib.sha256(k0.read_bytes()).hexdigest(),
                "samples_per_engine_phase": ROUNDS,
                "max_load_per_logical_cpu": MAX_LOAD_PER_LOGICAL_CPU,
                "method": "stock BenchmarkSuite.RunSuites; one-second warmup plus one-second measured phase; record exact existing n loop counter once per Measure; K0 uses the same runner Setup/TearDown with RunSingleBenchmark returning null; phase order alternates by round",
                "results": results,
            }
            (OUT / "stock-counters.json").write_text(json.dumps(report, indent=2) + "\n")
            pair = {sample["phase"]: sample for sample in results if sample["round"] == round_index and sample["engine"] == name}
            stock_result = pair["stock-work"]
            print(f"round={round_index + 1} engine={name} clean={all(sample['timing_clean'] for sample in pair.values())} work_exit={stock_result['exit_code']} calls={stock_result['total_run_calls']}", flush=True)


if __name__ == "__main__":
    main()
