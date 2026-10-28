"""Median Score and max RSS per engine from run-coremark.sh CSVs."""
import csv, statistics, sys
rows = [r for f in sys.argv[1:] for r in csv.DictReader(open(f))]
by = {}
for r in rows:
    if r["score"] != "NA":
        by.setdefault(r["engine"], []).append(r)
print(f"{'engine':16} {'n':>3} {'median score':>13} {'min..max':>17} {'median max RSS MB':>18}")
for e, rs in sorted(by.items(), key=lambda kv: -statistics.median(float(r["score"]) for r in kv[1])):
    s = [float(r["score"]) for r in rs]; m = [int(r["maxrss_kb"]) / 1024 for r in rs]
    print(f"{e:16} {len(rs):3} {statistics.median(s):13.1f} {min(s):8.0f}..{max(s):<8.0f} {statistics.median(m):18.2f}")
