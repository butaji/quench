#!/usr/bin/env python3
"""bench-board.py QUENCH [--benches richards,deltablue] [--runs 3] [--work DIR]

The comparison table every step reports (user, 2026-11-28): score and peak RSS of each V8 v7
bench in Quench no-jit, default and AOT (a compiled image) against Node, Bun and the C analogue
(quench-bench/c-analogues), all on this machine, median of RUNS runs each. Each JS engine runs
the pinned base.js + bench + the stock reporting runner; peak RSS is the child's ru_maxrss
(wait4). The C score is the harness formula, 100 * reference / microseconds per run, over the
analogue's timed iterations. Prints a Markdown table, with the build and run count, and
writes it to WORK/board.md.
"""
import argparse, os, re, statistics, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SUITE = os.path.join(ROOT, "quench-bench", "js-engine-benchmark", "v8-v7")
C_DIR = os.path.join(ROOT, "quench-bench", "c-analogues")
RUNNER = ('var r=[];BenchmarkSuite.RunSuites({NotifyResult:function(n,s){r.push(n+": "+s)},'
          'NotifyError:function(n,e){r.push("ERR "+n+" "+e)},NotifyScore:function(s){r.push("Score "+s)}});'
          'console.log(r.join("\\n"))')
CLEAN_ENV = {k: v for k, v in os.environ.items() if k not in ("BUN_OPTIONS", "NODE_OPTIONS", "QUENCH_TRACE")}


def run(argv, env=None):
    """(stdout, exit status, peak RSS in KiB) of one run."""
    proc = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env or CLEAN_ENV)
    out = proc.stdout.read().decode(errors="replace")
    _, status, usage = os.wait4(proc.pid, 0)
    proc.returncode = os.waitstatus_to_exitcode(status)
    return out, proc.returncode, usage.ru_maxrss


def js_score(out):
    m = re.search(r"^Score ([0-9.]+)$", out, re.M)
    return float(m.group(1)) if m and "ERR " not in out else None


def reference(bench):
    text = open(os.path.join(SUITE, bench + ".js")).read()
    m = re.search(r"new BenchmarkSuite\('[^']+',\s*\[?\s*([0-9]+)", text)
    return int(m.group(1))


def c_engine(bench, work):
    source = os.path.join(C_DIR, bench + ".c")
    if not os.path.exists(source):
        return None
    exe = os.path.join(work, bench + ".c.exe")
    if subprocess.run(["cc", "-O2", "-o", exe, source, "-lm"], capture_output=True).returncode != 0:
        return None
    iterations = int(re.search(r"// iterations: ([0-9]+)", open(source).read()).group(1))
    inner = int(m.group(1)) if (m := re.search(r"for \(int j = 0; j < ([0-9]+); j\+\+\)", open(source).read())) else 1
    ref = reference(bench)

    def score(out):
        m = re.search(r"([0-9.]+) ms", out)
        if not m:
            return None
        usec = float(m.group(1)) * 1000 / (iterations * inner)
        return 100 * ref / usec
    return [exe], score


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("quench")
    ap.add_argument("--benches", default="richards")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--work", default=None)
    ap.add_argument("--node", default="node")
    ap.add_argument("--bun", default="bun")
    a = ap.parse_args()
    work = os.path.abspath(a.work or tempfile.mkdtemp(prefix="board-"))
    os.makedirs(work, exist_ok=True)
    quench = os.path.abspath(a.quench)
    env = dict(CLEAN_ENV, XDG_CACHE_HOME=os.path.join(work, "xdg"), QUENCH_STARTUP_SNAPSHOT="off")
    rows = []
    for bench in a.benches.split(","):
        program = os.path.join(work, bench + ".js")
        with open(program, "w") as f:
            f.write(open(os.path.join(SUITE, "base.js")).read() + open(os.path.join(SUITE, bench + ".js")).read() + RUNNER + "\n")
        image = os.path.join(work, bench + ".aot")
        compiled = subprocess.run([quench, "compile", program, "-o", image], capture_output=True, env=env).returncode == 0
        engines = [
            ("Quench no-jit", [quench, "--no-jit", program], js_score, env),
            ("Quench default", [quench, program], js_score, env),
            ("Quench AOT", [image] if compiled else None, js_score, env),
            ("Node", [a.node, program], js_score, None),
            ("Bun", [a.bun, program], js_score, None),
        ]
        c = c_engine(bench, work)
        engines.append(("C (cc -O2)", c[0] if c else None, c[1] if c else None, None))
        for name, argv, score_of, engine_env in engines:
            if argv is None:
                rows.append((bench, name, None, None, "not built"))
                continue
            scores, rss, notes = [], [], set()
            for _ in range(a.runs):
                out, status, peak = run(argv, engine_env)
                s = score_of(out)
                if status != 0 or s is None:
                    notes.add(f"exit {status}" if status else "no score")
                    continue
                scores.append(s)
                rss.append(peak / 1024)
            rows.append((bench, name, statistics.median(scores) if scores else None, statistics.median(rss) if rss else None,
                         ", ".join(sorted(notes))))
    build = subprocess.run(["git", "-C", ROOT, "log", "-1", "--format=%h"], capture_output=True, text=True).stdout.strip()
    lines = [f"Median of {a.runs} runs per cell; Quench binary {quench} (tree {build}); same machine; "
             "score higher is better, peak RSS lower is better.", "",
             "| bench | engine | score | peak RSS (MB) | note |", "|---|---|---:|---:|---|"]
    for bench, name, score, rss, note in rows:
        lines.append(f"| {bench} | {name} | {score:,.1f} | {rss:,.1f} | {note} |" if score is not None
                     else f"| {bench} | {name} | - | - | {note} |")
    table = "\n".join(lines)
    print(table)
    open(os.path.join(work, "board.md"), "w").write(table + "\n")


if __name__ == "__main__":
    main()
