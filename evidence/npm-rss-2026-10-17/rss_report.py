import json, sys
r = json.load(open(sys.argv[1]))
rows = []
for x in r['records']:
    name = x['package']['name']
    def ok(o): return o and o['exit_code'] == 0 and not o['timed_out'] and o['signal'] is None
    q, n, b = x['quench'], x['node'], x.get('bun')
    rq = q['peak_rss_bytes'] / 2**20
    rn = n['peak_rss_bytes'] / 2**20
    rb = b['peak_rss_bytes'] / 2**20 if b and b['peak_rss_bytes'] else None
    others = [v for v, o in ((rn, n), (rb, b)) if v is not None and ok(o)]
    lowest = all(rq <= v for v in others)
    rows.append((name, x['verdict'], rq, rn, rb, ok(b), lowest))
lost = [row for row in rows if not row[6]]
print(f"packages {len(rows)}  quench lowest {len(rows)-len(lost)}  not lowest {len(lost)}")
for name, verdict, rq, rn, rb, bok, _ in sorted(lost, key=lambda row: row[2] - min(v for v in (row[3], row[4]) if v), reverse=True):
    print(f"  {name:40s} {verdict:18s} quench {rq:7.1f}  node {rn:7.1f}  bun {rb if rb is None else round(rb,1)} {'' if bok else '(bun failed)'}")
import statistics
print('median MiB quench %.1f node %.1f bun %.1f' % (statistics.median(r[2] for r in rows), statistics.median(r[3] for r in rows), statistics.median(r[4] for r in rows if r[4])))
