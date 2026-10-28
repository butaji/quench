#!/usr/bin/env python3
"""Collect executed object-literal key-count sites from V8-v7 fixtures."""

import hashlib
import json
import os
import platform
import subprocess
import tempfile
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BINARY = ROOT / "target/candidate-a/iteration/quench-node"
INPUTS = ROOT / "target/iteration/v8-v7-opcode-census-2026-10-08"
OUTPUT = ROOT / "tasks/evidence/task61-object-literal-screen-2026-10-09/site-census.json"
FIXTURES = (
    "crypto",
    "deltablue",
    "earley-boyer",
    "navier-stokes",
    "raytrace",
    "regexp",
    "richards",
    "splay",
)
RUNNER_MARKER = "let __quenchBenchSucceeded = true;"
RUNNER_TEMPLATE = """(function() {
  var suites = BenchmarkSuite.suites;
  for (var suiteIndex = 0; suiteIndex < suites.length; suiteIndex++) {
    var benchmarks = suites[suiteIndex].benchmarks;
    for (var benchmarkIndex = 0; benchmarkIndex < benchmarks.length; benchmarkIndex++) {
      var benchmark = benchmarks[benchmarkIndex];
      benchmark.Setup();
      if (RUN_BENCHMARKS) {
        benchmark.run();
      }
    }
  }
})();
"""


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def run_fixture(name, run_benchmarks):
    source = INPUTS / f"{name}.js"
    source_text = source.read_text()
    runner_position = source_text.rfind(RUNNER_MARKER)
    if runner_position < 0:
        raise RuntimeError(f"{name} is missing the expected benchmark runner")
    runner = RUNNER_TEMPLATE.replace("RUN_BENCHMARKS", str(run_benchmarks).lower())
    census_input = source_text[:runner_position] + runner
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=f"-{name}.js", dir=OUTPUT.parent, delete=False
    ) as temporary:
        temporary.write(census_input)
        census_path = Path(temporary.name)
    try:
        result = subprocess.run(
            [str(BINARY), str(census_path)],
            cwd=ROOT,
            env={**os.environ, "QUENCH_OPCODE_CENSUS": "1"},
            capture_output=True,
            text=True,
            check=False,
        )
    finally:
        census_path.unlink(missing_ok=True)
    if result.returncode:
        raise RuntimeError(f"{name} exited {result.returncode}: {result.stderr[-4000:]}")
    reports = [json.loads(line) for line in result.stderr.splitlines() if line.startswith('{')]
    by_kind = {report.get("kind"): report for report in reports}
    required = ("quench-dispatch-opcode-census", "quench-object-literal-sites")
    if any(kind not in by_kind for kind in required):
        raise RuntimeError(f"{name} missing reports: {by_kind.keys()}")
    return {
        "input_path": str(source.relative_to(ROOT)),
        "input_sha256": sha256(source.read_bytes()),
        "runner_mode": "one setup per benchmark, with one run call when enabled",
        "ran_benchmarks": run_benchmarks,
        "executed_input_sha256": sha256(census_input.encode()),
        "exit_code": result.returncode,
        "stdout_sha256": sha256(result.stdout.encode()),
        "global_dispatch_total": by_kind[required[0]]["total"],
        "global_dispatch_site_total": by_kind[required[0]]["site_total"],
        "global_object_opcode_counts": {
            name: by_kind[required[0]]["counts"].get(name, 0)
            for name in ("MakeObject2", "MakeObjectLiteral", "SuperConstArrayObject2")
        },
        "object_dispatch_counts": by_kind[required[1]]["dispatch_counts"],
        "sites": by_kind[required[1]]["sites"],
    }


def summarize(sites, execution_field):
    totals = {}
    for site in sites:
        key_count = str(site["key_count"])
        entry = totals.setdefault(key_count, {"executions": 0, "static_sites": 0, "executed_sites": 0})
        entry["static_sites"] += 1
        entry["executions"] += site[execution_field]
        entry["executed_sites"] += site[execution_field] > 0
    return totals


def check_opcode_totals(fixture, phase):
    observed = Counter()
    for site in fixture["sites"]:
        observed[site["dispatch_op"]] += site[f"{phase}_executions"]
    counts = fixture["phases"][phase]["object_dispatch_counts"]
    names = ("MakeObject2", "MakeObjectLiteral", "SuperConstArrayObject2")
    return {
        name: {"opcode_count": counts.get(name, 0), "site_count": observed[name]}
        for name in names
    }


def main():
    if not BINARY.is_file():
        raise SystemExit(f"missing aggregate-profile binary: {BINARY}")
    fixtures = {}
    for name in FIXTURES:
        setup = run_fixture(name, False)
        setup_and_run = run_fixture(name, True)
        by_key = {
            (site["function"], site["pc"]): site
            for site in setup["sites"]
        }
        sites = []
        for total_site in setup_and_run["sites"]:
            key = (total_site["function"], total_site["pc"])
            setup_site = by_key.get(key, {"executions": 0})
            sites.append(
                {
                    **total_site,
                    "setup_executions": setup_site["executions"],
                    "benchmark_run_executions": total_site["executions"]
                    - setup_site["executions"],
                }
            )
        fixture = {
            "input_path": setup["input_path"],
            "input_sha256": setup["input_sha256"],
            "phases": {"setup": setup, "setup_and_run": setup_and_run},
            "sites": sites,
        }
        for phase, phase_result in fixture["phases"].items():
            phase_sites = []
            for site in phase_result["sites"]:
                phase_sites.append({**site, f"{phase}_executions": site["executions"]})
            phase_result["sites"] = phase_sites
            reconciliation = check_opcode_totals(
                {"sites": phase_sites, "phases": {phase: phase_result}}, phase
            )
            if any(
                values["opcode_count"] != values["site_count"]
                for values in reconciliation.values()
            ):
                raise RuntimeError(f"{name}/{phase}: object-site executions do not reconcile")
            phase_result["site_opcode_reconciliation"] = reconciliation
        fixture["setup_key_count_totals"] = summarize(sites, "setup_executions")
        fixture["benchmark_run_key_count_totals"] = summarize(
            sites, "benchmark_run_executions"
        )
        fixtures[name] = fixture
    report = {
        "schema": "quench.task61.object-literal-site-census.v1",
        "date": "2026-10-09",
        "purpose": "Executed static object-literal sites by literal key count, separating benchmark setup from one benchmark.run() call per benchmark; profiling only, not timing.",
        "host": {"platform": platform.platform(), "machine": platform.machine()},
        "binary_path": str(BINARY.relative_to(ROOT)),
        "binary_sha256": sha256(BINARY.read_bytes()),
        "profile_feature": "quench-runtime/profile-aggregate",
        "profile_environment": {"QUENCH_OPCODE_CENSUS": "1"},
        "fixtures": fixtures,
    }
    OUTPUT.write_text(json.dumps(report, indent=2) + "\n")
    print(OUTPUT.relative_to(ROOT))
    for name, fixture in fixtures.items():
        print(
            f"{name}: setup={fixture['setup_key_count_totals']}; "
            f"benchmark_run={fixture['benchmark_run_key_count_totals']}"
        )


if __name__ == "__main__":
    main()
