#!/usr/bin/env python3
"""Per-structure table from run.py's results/<tag>.jsonl: geomean ms of the cases every engine
finished, peak RSS, and Quench's ratios to the better of Node and Bun (time and RSS).
usage: table.py results/TAG.jsonl"""
import json, math, sys, collections
cells = collections.defaultdict(dict)
for line in open(sys.argv[1]):
    c = json.loads(line)
    cells[(c["ds"], c["case"])][c["engine"]] = c
engines = ["c", "node", "bun", "q-no-jit", "q-b-jit", "q-default"]
byds = collections.defaultdict(list)
for (ds, case), e in cells.items():
    byds[ds].append(e)
gm = lambda xs: math.exp(sum(map(math.log, xs)) / len(xs))
print("| structure | cases | " + " | ".join(f"{e} ms" for e in engines) + " | best peer | no-jit | b-jit | default | RSS MB c/node/bun/q-default |")
print("|" + "---|" * (len(engines) + 8))
for ds, rows in byds.items():
    done = [r for r in rows if all(e in r and "ns" in r[e] for e in engines)]
    if not done: continue
    ms = {e: gm([r[e]["ns"] / 1e6 for r in done]) for e in engines}
    rss = {e: max(r[e]["rss_kib"] for r in done) / 1024 for e in engines}
    peer = min(ms["node"], ms["bun"]); peer_rss = min(rss["node"], rss["bun"])
    ratio = lambda e: f"{ms[e]/peer:.1f}x t, {rss[e]/peer_rss:.2f}x rss"
    print(f"| {ds} | {len(done)} | " + " | ".join(f"{ms[e]:.1f}" for e in engines) +
          f" | {'node' if ms['node'] <= ms['bun'] else 'bun'} | {ratio('q-no-jit')} | {ratio('q-b-jit')} | {ratio('q-default')} | "
          f"{rss['c']:.0f}/{rss['node']:.0f}/{rss['bun']:.0f}/{rss['q-default']:.0f} |")
