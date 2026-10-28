# Median score and max RSS per engine/mode from run-coremark.sh CSV; per-round win counts for Quench vs the best bar.
import csv, statistics as st, sys, collections
rows = [r for r in csv.DictReader(open(sys.argv[1])) if r["score"] not in ("NA", "")]
by = collections.defaultdict(list)
for r in rows: by[r["engine"]].append(r)
med = {e: (st.median(float(x["score"]) for x in v), st.median(int(x["maxrss_kb"]) for x in v), len(v),
           min(float(x["score"]) for x in v), max(float(x["score"]) for x in v)) for e, v in by.items()}
for mode in ("nojit", "jit"):
    print(f"\n== {mode} ==  engine | n | median score [min..max] | median max RSS MB")
    for e, (s, m, n, lo, hi) in sorted(((e, v) for e, v in med.items() if e.endswith(":" + mode)), key=lambda t: -t[1][0]):
        print(f"{e:28s} {n:2d}  {s:9.1f} [{lo:.0f}..{hi:.0f}]  {m/1024:6.2f}")
    bars = {e: v for e, v in med.items() if e.endswith(":" + mode) and not e.startswith("quench")}
    if not bars: continue
    best_s = max(bars.items(), key=lambda t: t[1][0]); low_m = min(bars.items(), key=lambda t: t[1][1])
    print(f"bars: Score {best_s[0]} {best_s[1][0]:.1f}; RSS {low_m[0]} {low_m[1][1]/1024:.2f} MB")
    rounds = collections.defaultdict(dict)
    for r in rows: rounds[r["round"]][r["engine"]] = r
    for q in sorted(e for e in med if e.startswith("quench") and e.endswith(":" + mode)):
        s, m = med[q][0], med[q][1]
        ahead = sum(1 for rr in rounds.values() if q in rr and best_s[0] in rr and float(rr[q]["score"]) > float(rr[best_s[0]]["score"]))
        print(f"  {q}: score {s/best_s[1][0]:.3f}x of bar ({'WIN' if s > best_s[1][0] else 'behind'}, ahead in {ahead}/{len(rounds)} rounds); "
              f"RSS {m/low_m[1][1]:.3f}x of bar ({'WIN' if m < low_m[1][1] else 'behind'})")
