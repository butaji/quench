#!/usr/bin/env python3
"""Writes the inputs of quench-gate's `ds-budgets` step from finished run.py and ir.py results.

    gate.py OUT_DIR --runs results/A.jsonl[,B.jsonl] --ir results/X.ir.jsonl

OUT_DIR/score.txt    `<mode> <structure> <score|FAIL> <peak_rss_kb>` for no-jit, b-jit, default
OUT_DIR/ir.txt       `<structure> <no-jit Ir per op, geomean of its cases>`
OUT_DIR/comparators.txt  `<node|bun> <structure> <score|FAIL> <peak_rss_kb>`
OUT_DIR/order.txt    `<structure>/<case> <mode> <ms> <ir_per_op|-> <peak_rss_kb|?>` (tier order, TOD-2152)
A structure's score is 1000 / (geomean ms of its cases), so higher is better as the step expects;
its RSS is the peak over its cases. A structure with any failed or mismatched case is FAIL.
Then: QUENCH_DS_SWEEP=OUT_DIR QUENCH_DS_COMPARATORS=OUT_DIR/comparators.txt quench-gate --only ds-budgets
"""
import argparse, collections, json, math
from pathlib import Path

MODES = {"q-no-jit": "no-jit", "q-b-jit": "b-jit", "q-default": "default"}
PEERS = ("node", "bun")


def load(paths):
    cells = {}
    for path in paths:
        for line in open(path):
            c = json.loads(line)
            cells[(c["engine"], c["ds"], c["case"])] = c  # a later run of a cell wins
    return cells


def per_structure(cells, engine):
    by = collections.defaultdict(list)
    for (eng, ds, _), c in cells.items():
        if eng == engine:
            by[ds].append(c)
    out = {}
    for ds, cs in by.items():
        ok = all("ns" in c and c.get("match_c", True) for c in cs)
        score = 1000 / math.exp(sum(math.log(c["ns"] / 1e6) for c in cs) / len(cs)) if ok else None
        rss = max((c.get("rss_kib") or 0) for c in cs) or None
        out[ds] = (score, rss)
    return out


def fmt(score, rss):
    return f"{'FAIL' if score is None else f'{score:.4f}'} {'?' if rss is None else rss}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--runs", required=True)
    ap.add_argument("--ir", required=True)
    a = ap.parse_args()
    out = Path(a.out); out.mkdir(parents=True, exist_ok=True)
    cells = load(a.runs.split(","))
    lines = []
    for engine, mode in MODES.items():
        for ds, (score, rss) in sorted(per_structure(cells, engine).items()):
            lines.append(f"{mode} {ds} {fmt(score, rss)}")
    (out / "score.txt").write_text("\n".join(lines + ["DONE"]) + "\n")
    lines = [f"{peer} {ds} {fmt(score, rss)}" for peer in PEERS
             for ds, (score, rss) in sorted(per_structure(cells, peer).items())]
    (out / "comparators.txt").write_text("\n".join(lines + ["DONE"]) + "\n")
    ir = collections.defaultdict(list)
    for line in open(a.ir):
        r = json.loads(line)
        if r["engine"] == "q-no-jit" and r.get("ir_per_op"):
            ir[r["ds"]].append(r["ir_per_op"])
    lines = [f"{ds} {math.exp(sum(map(math.log, v)) / len(v)):.1f}" for ds, v in sorted(ir.items())]
    (out / "ir.txt").write_text("\n".join(lines + ["DONE"]) + "\n")
    irs = {}
    for line in open(a.ir):
        r = json.loads(line)
        if r["engine"] in MODES and r.get("ir_per_op"):
            irs[(r["ds"], r["case"], r["engine"])] = r["ir_per_op"]
    lines = []
    for (engine, ds, case), c in sorted(cells.items()):
        if engine in MODES and "ns" in c and c.get("match_c", True):
            ir_op = irs.get((ds, case, engine))
            lines.append(f"{ds}/{case} {MODES[engine]} {c['ns'] / 1e6:.3f} {'-' if ir_op is None else ir_op} {c.get('rss_kib') or '?'}")
    (out / "order.txt").write_text("\n".join(lines + ["DONE"]) + "\n")
    print(f"wrote {out}/score.txt, ir.txt, comparators.txt, order.txt")


if __name__ == "__main__":
    main()
