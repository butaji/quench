#!/usr/bin/env python3
"""bench-board.py QUENCH [--base LABEL=QUENCH2] [--benches richards,deltablue] [--runs 3] [--work DIR]

--base puts a second Quench build beside the first in every section (each with its own target
row), so a regression and its closing show side by side.

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


def measure(argv, score_of, env, runs):
    """Median score, median peak RSS (MB), median wall seconds, and the failures, over `runs` runs."""
    import time
    scores, rss, walls, notes = [], [], [], set()
    for _ in range(runs):
        began = time.perf_counter()
        out, status, peak = run(argv, env)
        wall = time.perf_counter() - began
        s = score_of(out) if score_of else (0.0 if status == 0 else None)
        if status != 0 or s is None:
            notes.add(f"exit {status}" if status else "no score")
            continue
        scores.append(s); rss.append(peak / 1024); walls.append(wall)
    med = lambda xs: statistics.median(xs) if xs else None
    return med(scores), med(rss), med(walls), ", ".join(sorted(notes))


def fmt(value, digits=1):
    return "-" if value is None else f"{value:,.{digits}f}"


def ratio_cell(mine, theirs, target, higher):
    """`mine / theirs` against `target` (a ratio): pass when higher (or lower) than the target as `higher` says."""
    if mine is None or theirs is None or theirs == 0:
        return "-", "FAIL"
    r = mine / theirs
    ok = r >= target if higher else r <= target
    return f"{r:.3g}x", "pass" if ok else "FAIL"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("quench")
    ap.add_argument("--benches", default="richards")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--work", default=None)
    ap.add_argument("--node", default="node")
    ap.add_argument("--bun", default="bun")
    ap.add_argument("--no-aot", action="store_true", help="skip the AOT vs C rows (no image links)")
    ap.add_argument("--base", default=None, help="LABEL=PATH of a second Quench build shown beside the first")
    a = ap.parse_args()
    work = os.path.abspath(a.work or tempfile.mkdtemp(prefix="board-"))
    os.makedirs(work, exist_ok=True)
    quench = os.path.abspath(a.quench)
    build = subprocess.run(["git", "-C", ROOT, "log", "-1", "--format=%h"], capture_output=True, text=True).stdout.strip()
    builds = [(f"integ {build}", quench)]
    if a.base:
        label, path = a.base.split("=", 1)
        builds.append((label, os.path.abspath(path)))
    env = dict(CLEAN_ENV, XDG_CACHE_HOME=os.path.join(work, "xdg"), QUENCH_STARTUP_SNAPSHOT="off")
    bun_nojit = dict(CLEAN_ENV, BUN_JSC_useJIT="0")
    lines = [f"Median of {a.runs} runs per cell; Quench builds: {', '.join(l for l, _ in builds)}; one machine. Targets (user, 2026-11-28): "
             "no-jit and JIT vs the better of Node and Bun in the same mode, score >= 1.5x and peak RSS <= 0.5x; "
             "AOT vs C, parity (score >= 1x; RSS, startup, binary size <= 1x).", ""]
    # Startup: an image of an empty program against a C program that prints and exits.
    empty = os.path.join(work, "empty.js")
    open(empty, "w").write("console.log(1)\n")
    hello_c = os.path.join(work, "hello.c")
    open(hello_c, "w").write('#include <stdio.h>\nint main(void) { puts("1"); return 0; }\n')
    hello = os.path.join(work, "hello.exe")
    subprocess.run(["cc", "-O2", "-o", hello, hello_c], capture_output=True)
    start_q = {}
    for i, (label, q) in enumerate(builds if not a.no_aot else []):
        empty_image = os.path.join(work, f"empty{i}.aot")
        ok = subprocess.run([q, "compile", empty, "-o", empty_image], capture_output=True, env=env).returncode == 0
        start_q[label] = measure([empty_image], None, env, max(a.runs, 5))[2] if ok else None
    start_c = measure([hello], None, None, max(a.runs, 5))[2]
    for bench in a.benches.split(","):
        program = os.path.join(work, bench + ".js")
        with open(program, "w") as f:
            f.write(open(os.path.join(SUITE, "base.js")).read() + open(os.path.join(SUITE, bench + ".js")).read() + RUNNER + "\n")
        lines += [f"### {bench}", ""]
        for mode, flags, others in [
            ("no-jit", ["--no-jit"],
             [("Node --jitless", [a.node, "--jitless", program], None), ("Bun (BUN_JSC_useJIT=0)", [a.bun, program], bun_nojit)]),
            ("JIT", [],
             [("Node", [a.node, program], None), ("Bun", [a.bun, program], None)]),
        ]:
            rows = [(name, measure(argv, js_score, e, a.runs)) for name, argv, e in others]
            best_score = max((r[0] for _, r in rows if r[0] is not None), default=None)
            best_rss = min((r[1] for _, r in rows if r[1] is not None), default=None)
            mine = [(f"Quench {'--no-jit' if flags else 'default'} ({label})", measure([q] + flags + [program], js_score, env, a.runs))
                    for label, q in builds]
            lines += [f"| {mode} engine | score | peak RSS (MB) | note |", "|---|---:|---:|---|"]
            for name, r in mine + rows:
                lines.append(f"| {name} | {fmt(r[0])} | {fmt(r[1])} | {r[3]} |")
            for name, r in mine:
                sr, sv = ratio_cell(r[0], best_score, 1.5, True)
                rr, rv = ratio_cell(r[1], best_rss, 0.5, False)
                lines.append(f"| **target**, {name} | {sr} of best (1.5x): **{sv}** | {rr} of best (0.5x): **{rv}** | |")
            lines.append("")
        c = c_engine(bench, work)
        ca = measure(c[0], c[1], None, a.runs) if c else (None, None, None, "no analogue")
        size_c = os.path.getsize(c[0][0]) if c else None
        for i, (label, q) in enumerate(builds if not a.no_aot else []):
            image = os.path.join(work, f"{bench}{i}.aot")
            compiled = subprocess.run([q, "compile", program, "-o", image], capture_output=True, env=env).returncode == 0
            qa = measure([image], js_score, env, a.runs) if compiled else (None, None, None, "not built")
            size_q = os.path.getsize(image) if compiled else None
            sq = start_q[label]
            cells = [("score", qa[0], ca[0], 1.0, True, 1), ("peak RSS (MB)", qa[1], ca[1], 1.0, False, 1),
                     ("startup (ms)", sq and sq * 1000, start_c and start_c * 1000, 1.0, False, 2),
                     ("binary size (KB)", size_q and size_q / 1024, size_c and size_c / 1024, 1.0, False, 1)]
            lines += [f"| AOT vs C ({label}) | Quench AOT | C (cc -O2) | ratio | parity |", "|---|---:|---:|---:|---|"]
            for name, mine_v, c_v, target, higher, digits in cells:
                r, v = ratio_cell(mine_v, c_v, target, higher)
                lines.append(f"| {name} | {fmt(mine_v, digits)} | {fmt(c_v, digits)} | {r} | **{v}** |")
            if qa[3] or ca[3]:
                lines.append(f"| note | {qa[3]} | {ca[3]} | | |")
            lines.append("")
    lines.append("Startup is an image of `console.log(1)` against a C program that prints and exits; binary size is the bench's image against its C analogue.")
    table = "\n".join(lines)
    print(table)
    open(os.path.join(work, "board.md"), "w").write(table + "\n")


if __name__ == "__main__":
    main()
