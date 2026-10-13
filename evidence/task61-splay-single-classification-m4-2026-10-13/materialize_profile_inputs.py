#!/usr/bin/env python3
"""Materialize K=0/work CJS inputs for the Splay guard-path counter census.

Plan values mirror quench-bench/src/main.rs FIXED_WORK_PLANS. Each generated
program uses the same base.js + fixture + Setup/run/TearDown ordering.
"""

from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CORPUS = ROOT / "quench-bench/js-engine-benchmark/v8-v7"
OUTPUT = Path(__file__).resolve().parent
PLANS = {
    "richards.js": ("Richards", 1, 100),
    "regexp.js": ("RegExp", 1, 1),
}


def runner(suite: str, benchmark_count: int, iterations: int) -> str:
    return f'''\nvar __quenchFixedSuites = BenchmarkSuite.suites;
var __quenchFixedPrint = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
if (__quenchFixedSuites.length !== 1) throw new Error("fixed-work suite count mismatch");
var __quenchFixedSuite = __quenchFixedSuites[0];
if (__quenchFixedSuite.name !== "{suite}") throw new Error("fixed-work suite name mismatch");
if (__quenchFixedSuite.benchmarks.length !== {benchmark_count}) throw new Error("fixed-work benchmark count mismatch");
for (var __quenchFixedIndex = 0; __quenchFixedIndex < __quenchFixedSuite.benchmarks.length; __quenchFixedIndex++) {{
  var __quenchFixedBenchmark = __quenchFixedSuite.benchmarks[__quenchFixedIndex];
  __quenchFixedBenchmark.Setup();
  try {{
    for (var __quenchFixedRun = 0; __quenchFixedRun < {iterations}; __quenchFixedRun++) {{
      __quenchFixedBenchmark.run();
    }}
  }} finally {{
    __quenchFixedBenchmark.TearDown();
  }}
}}
__quenchFixedPrint("__quenchFixedWork:{suite}:{benchmark_count}:{iterations}");
'''


def main() -> None:
    prefix = (CORPUS / "base.js").read_text()
    for fixture, (suite, benchmark_count, work_iterations) in PLANS.items():
        source = prefix + (CORPUS / fixture).read_text()
        for label, iterations in (("k0", 0), (f"k{work_iterations}", work_iterations)):
            path = OUTPUT / f"{fixture.removesuffix('.js')}-{label}.js"
            path.write_text(source + runner(suite, benchmark_count, iterations))
            print(path)


if __name__ == "__main__":
    main()
