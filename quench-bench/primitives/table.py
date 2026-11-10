#!/usr/bin/env python3
"""Primitive-op table: ns/iteration per engine (taskset single core, median of RUNS processes).
usage: table.py OUT.json [op ...]   env: QN=path to quench-node, CORE=1, RUNS=7, N=5000000"""
import json, os, re, subprocess, statistics, sys
here = os.path.dirname(os.path.abspath(__file__))
qn = os.environ.get("QN", os.path.join(here, "../../target/release/quench-node"))
core, runs, n = os.environ.get("CORE", "1"), int(os.environ.get("RUNS", "7")), os.environ.get("N", "5000000")
inner = os.environ.get("INNER", "0")  # INNER=1000: many short calls (lane body entries) instead of one long call (OSR)
engines = {
  "default": ([qn], {}),
  "lane": ([qn], {"QUENCH_DEV": "js-lane-methods"}),
  "nojit": ([qn, "--no-jit"], {}),
  "node": (["node"], {}), "bun": (["bun"], {}),
}
only = os.environ.get("ENGINES", "").split(",") if os.environ.get("ENGINES") else list(engines)
names = re.findall(r"^ops\.(\w+) = ", open(os.path.join(here, "ops.js")).read(), re.M)
if len(sys.argv) > 2: names = sys.argv[2:]
res = {}
for op in names:
    res[op] = {}
    for eng in only:
        cmd, env = engines[eng]
        vals = []
        for _ in range(runs):
            e = {"PATH": os.environ["PATH"], "HOME": os.environ["HOME"], **env}
            try:
                p = subprocess.run(["taskset", "-c", core] + cmd + [os.path.join(here, "run.cjs"), op, n, "5", inner],
                                   capture_output=True, text=True, timeout=120, env=e)
                vals.append(json.loads(p.stdout.strip().splitlines()[-1])["min"])
            except Exception as ex:
                vals.append(None)
        ok = [v for v in vals if v is not None]
        res[op][eng] = round(statistics.median(ok), 3) if ok else None
    print(op, res[op], flush=True)
json.dump(res, open(sys.argv[1], "w"), indent=1)
