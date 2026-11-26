#!/usr/bin/env python3
"""Exact instructions (callgrind Ir) per operation inside the drivers' timed regions.

Quench: the driver is copied to driver.ir.mjs with `now()` also calling process.__irMark()
(QUENCH_DEV=ir-mark), and callgrind dumps before every quench_ir_mark. C: callgrind dumps
before every clock_gettime, which the C driver calls only from now(). Either way part 1 is
setup, part 2 the first timed region, part 3 untimed, and so on: the timed Ir is the sum of
the even parts. Ir/op = timed Ir / the driver's "ops".

usage: ir.py --engines c,q-no-jit,q-b-jit,q-default --ds unionfind --cases chain --n 20000
             [--quench PATH] [--reps 1] [--warmup 0] [--tag NAME]
"""
import argparse, glob, json, os, re, subprocess, sys, tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SUITE = HERE / "suite"
sys.path.insert(0, str(HERE))
from run import cases  # noqa: E402

MARKED_NOW = ("const __irMark = process.__irMark;\n"
              "const now = () => { __irMark(); return process.hrtime.bigint(); };")


def marked_driver(ds):
    src = (SUITE / ds / "node" / "driver.mjs").read_text()
    old = "const now = () => process.hrtime.bigint();"
    assert old in src, ds
    dst = SUITE / ds / "node" / "driver.ir.mjs"
    dst.write_text(src.replace(old, MARKED_NOW))
    return dst.name


def part_ir(path):
    for line in open(path):
        if line.startswith("summary:") or line.startswith("totals:"):
            return int(line.split()[1])
    return 0


def measure(engine, ds, case, n, reps, warmup, quench):
    out = Path(tempfile.mkdtemp(prefix=f"ir-{ds}-{case}-{engine}-"))
    base = ["valgrind", "--tool=callgrind", f"--callgrind-out-file={out}/cg.out"]
    env = dict(os.environ)
    if engine == "c":
        cwd = SUITE / ds / "c"
        if not (cwd / "driver_ir").exists():
            # valgrind does not decode every -march=native extension (AVX-512 traps as SIGILL).
            subprocess.run("cc -std=c17 -O3 -march=x86-64-v3 -flto -o driver_ir *.c", shell=True,
                           cwd=cwd, check=True)
        argv = base + ["--dump-before=*clock_gettime*", "./driver_ir"]
    else:
        mode = {"q-no-jit": ["--no-jit"], "q-b-jit": ["--b-jit"], "q-default": []}[engine]
        argv = base + ["--smc-check=all", "--dump-before=quench_ir_mark", quench, *mode,
                       "--expose-gc", marked_driver(ds)]
        cwd = SUITE / ds / "node"
        cache = HERE / "results" / ".xdg"
        cache.mkdir(parents=True, exist_ok=True)
        env.update(XDG_CACHE_HOME=str(cache), QUENCH_STARTUP_SNAPSHOT="off",
                   QUENCH_COMPILE_CACHE="", QUENCH_DEV="ir-mark")
    argv += [case, str(n), str(reps), str(warmup)]
    p = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True)
    rows = [json.loads(l) for l in p.stdout.splitlines() if l.startswith("{")]
    parts = sorted(glob.glob(f"{out}/cg.out.*"), key=lambda f: int(f.rsplit(".", 1)[1]))
    irs = [part_ir(f) for f in parts]
    # Parts are numbered from 1; the timed regions are parts 2, 4, ...; with warmup reps
    # the first 2*warmup*regions_per_rep are warm-up, so keep only the last reps' share.
    timed = irs[1::2]
    ops = sum(r["ops"] for r in rows)
    timed_ops = sum(r["ops"] for r in rows if not r["warmup"])
    regions = len(timed)
    keep = round(regions * timed_ops / ops) if ops else 0
    timed_ir = sum(timed[regions - keep:]) if keep else 0
    for f in glob.glob(f"{out}/cg.out*"):
        os.unlink(f)
    os.rmdir(out)
    return {"engine": engine, "ds": ds, "case": case, "n": n, "ops": timed_ops,
            "timed_ir": timed_ir, "ir_per_op": round(timed_ir / timed_ops, 1) if timed_ops else None,
            "regions": regions, "exit": p.returncode,
            "checksum": rows[-1]["checksum"] if rows else None,
            **({"error": p.stderr[-400:]} if p.returncode else {})}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--engines", default="c,q-no-jit,q-b-jit,q-default")
    ap.add_argument("--ds", required=True)
    ap.add_argument("--cases", default="")
    ap.add_argument("--n", type=int, default=20000)
    ap.add_argument("--reps", type=int, default=1)
    ap.add_argument("--warmup", type=int, default=0)
    ap.add_argument("--quench", default=str(HERE.parents[1] / "target/experiment/quench"))
    ap.add_argument("--tag", default="ir")
    a = ap.parse_args()
    (HERE / "results").mkdir(exist_ok=True)
    log = open(HERE / "results" / f"{a.tag}.ir.jsonl", "a")
    for ds in a.ds.split(","):
        for case in (a.cases.split(",") if a.cases else cases(ds)):
            for eng in a.engines.split(","):
                r = measure(eng, ds, case, a.n, a.reps, a.warmup, a.quench)
                log.write(json.dumps(r) + "\n"); log.flush()
                print(f"{ds}\t{case}\t{eng}\t{r['ir_per_op']}\tregions={r['regions']}"
                      f"\t{r.get('error','')[:100]}", flush=True)


if __name__ == "__main__":
    main()
