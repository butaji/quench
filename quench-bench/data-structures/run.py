#!/usr/bin/env python3
"""Runs the Gemini 4 Neon data-structure suite (see fetch.sh) on C, Node, Bun and Quench.

Like upstream, each (structure, case) cell is one process: `driver <case> <n> <reps> <warmup>`,
the median of the timed repetitions is kept, and a structure's time is the geometric mean of
its ten cases. Peak RSS comes from wait4 on that process. Every engine's checksum and verify
values must equal C's for the same n.

usage: run.py [--engines c,node,bun,q-no-jit,q-default] [--ds btree,...]
              [--scale 1.0] [--reps 2] [--warmup 1] [--quench PATH] [--tag NAME]
Writes results/<tag>.jsonl (one line per cell) and prints a per-structure table.
"""
import argparse, json, math, os, subprocess, sys, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
SUITE = HERE / "suite"
RSS_EXEC = HERE / "rss_exec"  # cc -O2 -o rss_exec rss_exec.c
# Upstream sizes (https://ben-chmarks.com/gemini-neon/).
SIZES = {"btree": 4_000_000, "dlist": 3_000_000, "hashmap": 2_000_000, "heap": 3_000_000,
         "radix": 1_500_000, "rbtree": 1_500_000, "rtree": 400_000, "skiplist": 1_250_000,
         "unionfind": 4_000_000}
JSFLAGS = ["--expose-gc", "--max-old-space-size=4096"]


def cases(ds):
    src = (SUITE / ds / "node" / "driver.mjs").read_text()
    out = []
    for line in src.splitlines():
        line = line.strip()
        if line.startswith('case "') and "return" in line:
            out.append(line.split('"')[1])
    return out


def command(engine, ds, quench):
    if engine == "c":
        return [str(SUITE / ds / "c" / "driver")], SUITE / ds / "c"
    if engine == "node":
        return ["node", *JSFLAGS, "driver.mjs"], SUITE / ds / "node"
    if engine == "bun":
        return ["bun", "driver.mjs"], SUITE / ds / "bun"
    mode = engine[2:]
    flags = {"no-jit": ["--no-jit"], "default": []}[mode]
    return [quench, *flags, *JSFLAGS, "driver.mjs"], SUITE / ds / "node"


def run_cell(engine, ds, case, n, reps, warmup, quench):
    argv, cwd = command(engine, ds, quench)
    argv = argv + [case, str(n), str(reps), str(warmup)]
    env = dict(os.environ)
    if engine.startswith("q-"):
        cache = HERE / "results" / ".xdg"
        cache.mkdir(parents=True, exist_ok=True)
        env.update(XDG_CACHE_HOME=str(cache), QUENCH_STARTUP_SNAPSHOT="off",
                   QUENCH_COMPILE_CACHE="")
    t0 = time.monotonic()
    rss_file = HERE / "results" / ".rss"
    rss_file.unlink(missing_ok=True)
    env["RSS_OUT"] = str(rss_file)
    p = subprocess.Popen([str(RSS_EXEC), *argv], cwd=cwd, env=env, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, text=True)
    out, err = p.communicate()
    wall = time.monotonic() - t0
    rss = rss_file.read_text().strip() if rss_file.exists() else ""
    rss_kib = int(rss) if rss else None
    rows = []
    for line in out.splitlines():
        if line.startswith("{"):
            rows.append(json.loads(line))
    timed = [r for r in rows if not r["warmup"]]
    cell = {"engine": engine, "ds": ds, "case": case, "n": n, "wall_s": round(wall, 3),
            "rss_kib": rss_kib, "exit": p.returncode}
    if p.returncode != 0 or not timed:
        cell["error"] = (err or out)[-600:]
        return cell
    ns = sorted(r["ns"] for r in timed)
    cell.update(ns=ns[len(ns) // 2] if len(ns) % 2 else (ns[len(ns)//2-1] + ns[len(ns)//2]) / 2,
                ops=timed[0]["ops"], checksum=timed[0]["checksum"], verify=timed[0]["verify"])
    return cell


def geomean(xs):
    return math.exp(sum(math.log(x) for x in xs) / len(xs)) if xs else float("nan")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--engines", default="c,node,bun,q-no-jit,q-default")
    ap.add_argument("--ds", default=",".join(SIZES))
    ap.add_argument("--cases", default="")
    ap.add_argument("--scale", type=float, default=1.0)
    ap.add_argument("--reps", type=int, default=2)
    ap.add_argument("--warmup", type=int, default=1)
    ap.add_argument("--quench", default=str(HERE.parents[1] / "target/experiment/quench"))
    ap.add_argument("--tag", default=time.strftime("%Y%m%d-%H%M%S"))
    a = ap.parse_args()
    engines = a.engines.split(",")
    (HERE / "results").mkdir(exist_ok=True)
    out = open(HERE / "results" / f"{a.tag}.jsonl", "a")
    ref = {}
    table = {}
    for ds in a.ds.split(","):
        n = max(1, int(SIZES[ds] * a.scale))
        for case in cases(ds):
            if a.cases and case not in a.cases.split(","):
                continue
            for eng in engines:
                cell = run_cell(eng, ds, case, n, a.reps, a.warmup, a.quench)
                if eng == "c" and "checksum" in cell:
                    ref[(ds, case)] = (cell["checksum"], cell["verify"])
                if (ds, case) in ref and "checksum" in cell and eng != "c":
                    cell["match_c"] = ref[(ds, case)] == (cell["checksum"], cell["verify"])
                out.write(json.dumps(cell) + "\n"); out.flush()
                table.setdefault((ds, eng), []).append(cell)
                tag = cell.get("error", "")[:120].replace("\n", " ")
                print(f"{ds:9} {case:20} {eng:10} ns={cell.get('ns','-'):>12} "
                      f"rss={cell['rss_kib']} match={cell.get('match_c','-')} {tag}",
                      file=sys.stderr, flush=True)
    print("ds\t" + "\t".join(f"{e}_ms\t{e}_rssMB" for e in engines))
    for ds in a.ds.split(","):
        row = [ds]
        for e in engines:
            cells = table.get((ds, e), [])
            ok = [c for c in cells if "ns" in c]
            ms = geomean([c["ns"] / 1e6 for c in ok]) if len(ok) == len(cells) and ok else float("nan")
            rss = max((c["rss_kib"] or 0) for c in cells) / 1024 if cells else float("nan")
            row += [f"{ms:.1f}", f"{rss:.0f}"]
        print("\t".join(row))


if __name__ == "__main__":
    main()
