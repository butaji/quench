#!/usr/bin/env python3
"""Self Ir per function, summed over callgrind dump files (uncompressed names:
run valgrind with --compress-strings=no). usage: cgself.py FILE... [-n 30]"""
import sys, collections
files = [a for a in sys.argv[1:] if not a.startswith("-n")]
top = 30
if "-n" in sys.argv:
    top = int(sys.argv[sys.argv.index("-n") + 1]); files = [f for f in files if f != str(top)]
cost = collections.Counter(); total = 0
for path in files:
    fn = None; skip = False
    for line in open(path):
        if line.startswith("fn="):
            fn = line[3:].strip(); continue
        if line.startswith("calls="):
            skip = True; continue
        if line[:1].isdigit() or line[:1] in "+-*":
            parts = line.split()
            if len(parts) >= 2:
                if skip:
                    skip = False; continue
                c = int(parts[1]); cost[fn] += c; total += c
for fn, c in cost.most_common(top):
    print(f"{c:>14,} {100*c/total:5.1f}%  {fn[:150]}")
print(f"{total:>14,} total")
