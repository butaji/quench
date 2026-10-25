# usage: ratios.py SWEEP RIVAL_NAME ; quench row first, rival second per test
import re, math, sys
q = {}; b = {}
for l in open(sys.argv[1]):
    m = re.match(r"(\S+)\s+(\S+)\s+score=(\S+)\s+maxrss=(\d+)", l)
    if not m: continue
    d = b if m[2] == sys.argv[2] else q
    try: d[m[1]] = (float(m[3]), int(m[4]))
    except ValueError: d[m[1]] = None
rows = [(q[t][0] / b[t][0], t, b[t][1] / q[t][1]) for t in q if q[t] and b.get(t)]
for s, t, r in sorted(rows):
    print(f"{t:28} {q[t][0]:8.2f} {b[t][0]:8.2f} score x{s:.3f} rss x{r:.2f}{'' if s >= 1.2 and r >= 1.2 else ' <'}")
n = len(rows)
print(f"geomean score x{math.exp(sum(math.log(s) for s,_,_ in rows)/n):.3f} rss x{math.exp(sum(math.log(r) for _,_,r in rows)/n):.2f} n={n} "
      f"score>=1.2: {sum(s >= 1.2 for s,_,_ in rows)} rss>=1.2: {sum(r >= 1.2 for _,_,r in rows)} both: {sum(s >= 1.2 and r >= 1.2 for s,_,r in rows)}")
missing = [t for t in q if not q[t] or not b.get(t)]
if missing: print("failed:", missing)
