#!/usr/bin/env python3
"""Measure GC phase share during fixed-work Splay runs on macOS."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import re
import statistics
import subprocess
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
SUITE = ROOT / "quench-bench/js-engine-benchmark/v8-v7"
OUT = pathlib.Path(__file__).resolve().parent
PROFILE_BINARY_SHA256 = "a486ee1e7698546fc5d8427dd7e48a5904e676ebe9bfab6d643c5e36e67dc746"
DEFAULT_BINARY = ROOT / "target/pinned" / PROFILE_BINARY_SHA256 / "quench-node"
COMPLETE_REPORT = "gc-share-11.json"
INCOMPLETE_REPORT = "gc-share-incomplete.json"
ROUNDS = 11
RUNS = 300
MAX_LOAD_PER_CPU = 1.0
MAX_ATTEMPTS = 44
NANOSECONDS_PER_SECOND = 1_000_000_000
WARMUP_ROUND = -1
SETUP = "setup"
WORK = "work"
PHASE_RUNS = {SETUP: 0, WORK: RUNS}
GC_PHASES = ("mark", "sweep", "mark_clear")
MARKER = "__quenchFixedWork:Splay:1:"
RUNTIME_PATHS = (
    "crates/quench-runtime/Cargo.toml",
    "crates/quench-runtime/src/heap.rs",
    "crates/quench-runtime/src/heap/tests.rs",
    "crates/quench-runtime/src/vm.rs",
)
GC_PATTERN = re.compile(r'^\{"kind":"quench-gc-time".*\}$', re.MULTILINE)
WALL_PATTERN = re.compile(r"([0-9.]+) real\s+([0-9.]+) user\s+([0-9.]+) sys")
RSS_PATTERN = re.compile(r"([0-9,]+)\s+maximum resident set size")


def materialize(runs: int, path: pathlib.Path) -> None:
    base = (SUITE / "base.js").read_bytes()
    fixture = (SUITE / "splay.js").read_bytes()
    runner = f"""
var __quenchFixedSuites = BenchmarkSuite.suites;
var __quenchFixedPrint = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
if (__quenchFixedSuites.length !== 1) throw new Error("fixed-work suite count mismatch");
var __quenchFixedSuite = __quenchFixedSuites[0];
if (__quenchFixedSuite.name !== "Splay") throw new Error("fixed-work suite name mismatch");
if (__quenchFixedSuite.benchmarks.length !== 1) throw new Error("fixed-work benchmark count mismatch");
for (var __quenchFixedIndex = 0; __quenchFixedIndex < __quenchFixedSuite.benchmarks.length; __quenchFixedIndex++) {{
  var __quenchFixedBenchmark = __quenchFixedSuite.benchmarks[__quenchFixedIndex];
  __quenchFixedBenchmark.Setup();
  try {{
    for (var __quenchFixedRun = 0; __quenchFixedRun < {runs}; __quenchFixedRun++) {{
      __quenchFixedBenchmark.run();
    }}
  }} finally {{
    __quenchFixedBenchmark.TearDown();
  }}
}}
__quenchFixedPrint("{MARKER}{runs}");
""".encode()
    path.write_bytes(base + b"\n" + fixture + b"\n" + runner)


def load_per_cpu() -> tuple[list[float], int]:
    cpus = os.cpu_count() or 1
    return list(os.getloadavg()), cpus


def read_profile(sample: dict, process: subprocess.CompletedProcess[str]) -> dict:
    gc_match = GC_PATTERN.search(process.stderr)
    wall_match = WALL_PATTERN.search(process.stderr)
    rss_match = RSS_PATTERN.search(process.stderr)
    sample.update(
        {
            "gc": json.loads(gc_match.group(0)) if gc_match else None,
            "wall_seconds": float(wall_match.group(1)) if wall_match else None,
            "user_seconds": float(wall_match.group(2)) if wall_match else None,
            "system_seconds": float(wall_match.group(3)) if wall_match else None,
            "max_rss_bytes": int(rss_match.group(1).replace(",", ""))
            if rss_match
            else None,
            "valid": (
                process.returncode == 0
                and sample["marker_ok"]
                and gc_match is not None
                and wall_match is not None
            ),
        }
    )
    return sample


def run_one(binary: pathlib.Path, source: pathlib.Path, round_index: int, phase: str) -> dict:
    before, cpus = load_per_cpu()
    load_before_per_cpu = before[0] / cpus
    if load_before_per_cpu >= MAX_LOAD_PER_CPU:
        raise RuntimeError(
            f"timing refused before process: load/core={load_before_per_cpu:.2f}"
        )
    environment = os.environ.copy()
    environment["QUENCH_GC_TIME_PROFILE"] = "1"
    started_ns = time.time_ns()
    process = subprocess.run(
        ["/usr/bin/time", "-l", str(binary), str(source)],
        cwd=ROOT,
        env=environment,
        text=True,
        capture_output=True,
        check=False,
    )
    after, _ = load_per_cpu()
    sample = {
        "round": round_index,
        "phase": phase,
        "exit_code": process.returncode,
        "marker_ok": f"{MARKER}{PHASE_RUNS[phase]}" in process.stdout,
        "timing_clean": max(before[0], after[0]) / cpus < MAX_LOAD_PER_CPU,
        "load_before": before,
        "load_after": after,
        "logical_cpus": cpus,
        "elapsed_ns": time.time_ns() - started_ns,
        "stdout": process.stdout,
        "stderr": process.stderr,
    }
    return read_profile(sample, process)


def phase_delta(work: dict, setup: dict) -> dict:
    wall_ns = (work["wall_seconds"] - setup["wall_seconds"]) * NANOSECONDS_PER_SECOND
    phase_deltas = {
        name: work["gc"][f"{name}_nanos"] - setup["gc"][f"{name}_nanos"]
        for name in GC_PHASES
    }
    mark_sweep_ns = phase_deltas["mark"] + phase_deltas["sweep"]
    all_gc_ns = mark_sweep_ns + phase_deltas["mark_clear"]
    return {
        "marginal_wall_ns": wall_ns,
        "marginal_mark_ns": phase_deltas["mark"],
        "marginal_sweep_ns": phase_deltas["sweep"],
        "marginal_mark_clear_ns": phase_deltas["mark_clear"],
        "marginal_mark_sweep_ns": mark_sweep_ns,
        "marginal_mark_sweep_share": mark_sweep_ns / wall_ns if wall_ns > 0 else None,
        "marginal_all_gc_phases_ns": all_gc_ns,
        "marginal_all_gc_phases_share": all_gc_ns / wall_ns if wall_ns > 0 else None,
        "work_collections": work["gc"]["collections"],
        "setup_collections": setup["gc"]["collections"],
    }


def warm_up(binary: pathlib.Path, paths: dict[str, pathlib.Path]) -> tuple[list[dict], str | None]:
    samples = []
    for phase in (SETUP, WORK):
        try:
            sample = run_one(binary, paths[phase], WARMUP_ROUND, phase)
        except RuntimeError as error:
            return samples, str(error)
        samples.append(sample)
        if not sample["valid"] or not sample["timing_clean"]:
            return samples, f"{phase} warm-up failed validity or load check"
    return samples, None


def pair_samples(binary: pathlib.Path, paths: dict[str, pathlib.Path], attempt: int, round_index: int) -> tuple[dict, str | None]:
    order = (SETUP, WORK) if attempt % 2 else (WORK, SETUP)
    samples = {}
    for phase in order:
        try:
            samples[phase] = run_one(binary, paths[phase], round_index, phase)
        except RuntimeError as error:
            return samples, str(error)
        if not samples[phase]["timing_clean"]:
            break
    return samples, None


def pair_record(round_index: int, samples: dict, refused: str | None) -> dict:
    both_phases = len(samples) == len(PHASE_RUNS)
    timing_clean = both_phases and all(row["timing_clean"] for row in samples.values())
    processes_valid = both_phases and all(row["valid"] for row in samples.values())
    pair_valid = timing_clean and processes_valid
    marginal = phase_delta(samples[WORK], samples[SETUP]) if pair_valid else None
    error = None
    if marginal:
        if marginal["marginal_wall_ns"] <= 0:
            error = "work-minus-setup wall time must be positive"
        elif min(marginal[f"marginal_{phase}_ns"] for phase in GC_PHASES) < 0:
            error = "all work-minus-setup GC phase times must be nonnegative"
        elif marginal["work_collections"] < marginal["setup_collections"]:
            error = "work must not collect fewer times than setup-only"
        pair_valid = error is None
    record = {
        "round": round_index,
        "valid": pair_valid,
        "timing_clean": timing_clean,
        "processes_valid": processes_valid,
        "samples": samples,
    }
    if marginal is not None:
        record["marginal"] = marginal
    if error:
        record["marginal_validation_error"] = error
    if refused:
        record["refused"] = refused
    return record


def collect_pairs(binary: pathlib.Path, paths: dict[str, pathlib.Path]) -> tuple[list[dict], int]:
    rounds = []
    completed = 0
    attempts = 0
    while completed < ROUNDS and attempts < MAX_ATTEMPTS:
        attempts += 1
        samples, refused = pair_samples(binary, paths, attempts, completed)
        record = pair_record(completed, samples, refused)
        rounds.append(record)
        completed += int(record["valid"])
        if refused:
            break
    return rounds, attempts


def median_field(rows: list[dict], name: str) -> float | None:
    values = [row["marginal"][name] for row in rows]
    return statistics.median(values) if values else None


def runtime_source_state() -> tuple[str, bool]:
    patch = subprocess.check_output(
        ["git", "diff", "HEAD", "--binary", "--", *RUNTIME_PATHS], cwd=ROOT
    )
    return hashlib.sha256(patch).hexdigest(), bool(patch)


def input_hashes() -> dict[str, str]:
    inputs = {"base": SUITE / "base.js", "splay": SUITE / "splay.js"}
    return {
        f"{name}_sha256": hashlib.sha256(path.read_bytes()).hexdigest()
        for name, path in inputs.items()
    }


def build_report(binary: pathlib.Path, warmup: list[dict], warmup_error: str | None, rounds: list[dict], attempts: int) -> dict:
    clean_pairs = [row for row in rounds if row["valid"]]
    runtime_patch_hash, runtime_dirty = runtime_source_state()
    return {
        "schema": 2,
        "source_revision": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip(),
        "runtime_source_dirty": runtime_dirty,
        "runtime_patch_sha256": runtime_patch_hash,
        "profile_binary": str(binary),
        "profile_binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "measurement_script_sha256": hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
        "gc_feature": "profile-gc-time",
        "work_runs": RUNS,
        "rounds_requested": ROUNDS,
        "attempts": attempts,
        "valid_pairs": len(clean_pairs),
        "max_load_per_logical_cpu": MAX_LOAD_PER_CPU,
        "logical_cpus": os.cpu_count() or 1,
        "host": subprocess.check_output(["uname", "-a"], text=True).strip(),
        "input_hashes": input_hashes(),
        "warmup_samples": warmup,
        "warmup_error": warmup_error,
        "summary": {
            name: median_field(clean_pairs, name)
            for name in (
                "marginal_wall_ns",
                "marginal_mark_ns",
                "marginal_sweep_ns",
                "marginal_mark_clear_ns",
                "marginal_mark_sweep_share",
                "marginal_all_gc_phases_share",
            )
        },
        "rounds": rounds,
    }


def main() -> None:
    binary = pathlib.Path(os.environ.get("QUENCH_GC_PROFILE_BINARY", DEFAULT_BINARY)).resolve()
    if not binary.is_file():
        raise SystemExit(f"profile-gc-time binary not found: {binary}")
    paths = {WORK: OUT / "work-300.js", SETUP: OUT / "setup-0.js"}
    materialize(RUNS, paths[WORK])
    materialize(0, paths[SETUP])
    warmup, warmup_error = warm_up(binary, paths)
    rounds, attempts = collect_pairs(binary, paths) if warmup_error is None else ([], 0)
    report = build_report(binary, warmup, warmup_error, rounds, attempts)
    complete = report["valid_pairs"] == ROUNDS and warmup_error is None
    output = OUT / (COMPLETE_REPORT if complete else INCOMPLETE_REPORT)
    output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["summary"], indent=2))
    print(f"valid pairs: {report['valid_pairs']}/{ROUNDS}; report: {output}")
    if report["valid_pairs"] != ROUNDS:
        raise SystemExit("did not collect all 11 valid work/K=0 pairs")


if __name__ == "__main__":
    main()
