# usage: medcmp.py N TEST BIN... ; interleaves N rounds, prints median score and max RSS per binary
import sys, subprocess, statistics, re, os
S = os.path.dirname(os.path.abspath(sys.argv[0])); n = int(sys.argv[1]); test = sys.argv[2]; bins = sys.argv[3:]
res = {b: [] for b in bins}; rss = {b: 0 for b in bins}
for _ in range(n):
    for b in bins:
        out = subprocess.run([sys.executable, S + "/rsscmp.py", test, b], capture_output=True, text=True).stdout
        m = re.search(r"score=([0-9.]+)\s+maxrss=(\d+)", out)
        if m: res[b].append(float(m.group(1))); rss[b] = max(rss[b], int(m.group(2)))
for b in bins:
    v = res[b]
    print(f"{test:24} {os.path.basename(b):10} median={statistics.median(v) if v else 0:7.2f} min={min(v, default=0):7.2f} max={max(v, default=0):7.2f} rss={rss[b]}", flush=True)
