"""Diagnostic V8-v7 A/B: Score and peak RSS (wait4) of two quench-node binaries, interleaved.

usage: python3 -I v8_diag.py SUITE_DIR ROUNDS BEFORE AFTER [--jit]

Each fixture runs as the pinned suite's base.js, the fixture and the stock reporting runner
(as quench-bench/v8-v7-rss.cjs builds it). Prints one line per fixture with the median Score
and median peak RSS of each binary. A diagnostic, not a qualification run.
"""
import os
import re
import statistics
import subprocess
import sys
import tempfile

FIXTURES = ["richards", "deltablue", "crypto", "raytrace", "earley-boyer", "regexp", "splay", "navier-stokes"]
RUNNER = """
let __ok = true;
const __print = typeof console !== "undefined" ? console.log.bind(console) : print;
BenchmarkSuite.RunSuites({
  NotifyResult(name, result) {},
  NotifyError(name, error) { __ok = false; __print(name + ": " + error); },
  NotifyScore(score) { if (__ok) __print("Score: " + score); },
});
"""


def run(binary, script, extra):
    process = subprocess.Popen([binary, *extra, script], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    output = process.stdout.read().decode()
    _, status, usage = os.wait4(process.pid, 0)
    match = re.search(r"^Score:\s*([\d.]+)$", output, re.M)
    if status != 0 or not match:
        raise SystemExit(f"{binary} {script}: status {status}, output {output[-200:]!r}")
    return float(match.group(1)), usage.ru_maxrss


def main():
    suite, rounds, before, after = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
    extra = ["--jit"] if "--jit" in sys.argv[5:] else []
    base = open(os.path.join(suite, "base.js")).read()
    with tempfile.TemporaryDirectory() as directory:
        for fixture in FIXTURES:
            script = os.path.join(directory, fixture + ".js")
            with open(script, "w") as out:
                out.write(base + "\n" + open(os.path.join(suite, fixture + ".js")).read() + RUNNER)
            samples = {"before": [], "after": []}
            for _ in range(rounds):
                for name, binary in (("before", before), ("after", after)):
                    samples[name].append(run(binary, script, extra))
            row = [fixture]
            for name in ("before", "after"):
                scores = [score for score, _ in samples[name]]
                rss = [kib for _, kib in samples[name]]
                row.append(f"{name} score {statistics.median(scores):.0f} rss {statistics.median(rss) / 1024:.1f} MiB")
            print(" | ".join(row), flush=True)


main()
