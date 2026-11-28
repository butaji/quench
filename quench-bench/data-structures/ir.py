#!/usr/bin/env python3
"""Exact instructions (callgrind Ir) per operation, by whole-run differencing.

Each (engine, case) runs under callgrind twice: the driver at n and at 2n, with the same
warm-up, so start-up, warm-up and per-process costs cancel: Ir/iteration = (Ir(2n) - Ir(n)) / n.
Ir/op divides that by the ops the two runs add (the driver's "ops" in each run's rows).
This is the method of crates/quench-gate/src/ir_budget.rs. Quench runs the Node driver with
QUENCH_DEV=sync-compile in the JIT modes, so a compile counts where it is made; C runs the
driver built with -march=x86-64-v3, as valgrind traps on AVX-512.

usage: ir.py --engines c,q-no-jit,q-b-jit,q-default --ds unionfind --cases chain --n 20000
             [--quench PATH] [--reps 1] [--warmup 0] [--tag NAME]
"""
import argparse, json, os, subprocess, sys, tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SUITE = HERE / "suite"
sys.path.insert(0, str(HERE))
from run import cases  # noqa: E402


def total_ir(path):
    for line in open(path):
        if line.startswith("summary:") or line.startswith("totals:"):
            return int(line.split()[1])
    return 0


def whole_run(engine, ds, case, length, reps, warmup, quench):
    """One callgrind run of the driver at `length`: its total Ir, its ops, and its exit status."""
    out = Path(tempfile.mkdtemp(prefix=f"ir-{ds}-{case}-{engine}-"))
    cg = out / "cg.out"
    argv = ["valgrind", "--tool=callgrind", "--smc-check=all", "--fair-sched=yes",
            f"--callgrind-out-file={cg}"]
    env = dict(os.environ)
    if engine == "c":
        cwd = SUITE / ds / "c"
        if not (cwd / "driver_ir").exists():
            # valgrind does not decode every -march=native extension (AVX-512 traps as SIGILL).
            subprocess.run("cc -std=c17 -O3 -march=x86-64-v3 -flto -o driver_ir *.c", shell=True,
                           cwd=cwd, check=True)
        argv += ["./driver_ir"]
    else:
        mode = {"q-no-jit": ["--no-jit"], "q-b-jit": ["--b-jit"], "q-default": []}[engine]
        argv += [quench, *mode, "--expose-gc", "driver.mjs"]
        cwd = SUITE / ds / "node"
        cache = HERE / "results" / ".xdg"
        cache.mkdir(parents=True, exist_ok=True)
        env.update(XDG_CACHE_HOME=str(cache), QUENCH_STARTUP_SNAPSHOT="off", QUENCH_COMPILE_CACHE="")
        if engine != "q-no-jit":
            # The JIT tiers compile on the requesting thread, so a compile counts where it is made.
            env["QUENCH_DEV"] = "sync-compile"
    argv += [case, str(length), str(reps), str(warmup)]
    p = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True)
    rows = [json.loads(l) for l in p.stdout.splitlines() if l.startswith("{")]
    ir = total_ir(cg) if cg.exists() else 0
    for f in out.glob("cg.out*"):
        f.unlink()
    out.rmdir()
    return {"ir": ir, "ops": sum(r["ops"] for r in rows), "rows": rows, "exit": p.returncode,
            "stderr": p.stderr[-400:]}


def measure(engine, ds, case, n, reps, warmup, quench):
    short = whole_run(engine, ds, case, n, reps, warmup, quench)
    long = whole_run(engine, ds, case, 2 * n, reps, warmup, quench)
    failed = short["exit"] or long["exit"]
    ops = long["ops"] - short["ops"]
    ir = long["ir"] - short["ir"]
    return {"engine": engine, "ds": ds, "case": case, "n": n, "ops": ops, "ir": ir,
            "ir_per_op": round(ir / ops, 1) if ops > 0 and not failed else None,
            "exit": failed,
            "checksum": long["rows"][-1]["checksum"] if long["rows"] else None,
            **({"error": (short["stderr"] or long["stderr"])} if failed else {})}


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
                print(f"{ds}\t{case}\t{eng}\t{r['ir_per_op']}\tops={r['ops']}"
                      f"\t{r.get('error','')[:100]}", flush=True)


if __name__ == "__main__":
    main()
