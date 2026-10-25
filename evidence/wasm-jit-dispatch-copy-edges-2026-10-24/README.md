# Wasm JIT: dispatch copies charged by table edges; unreachable admitted at every size

Host: Linux x86_64 cloud container (4 CPUs), JetStream 3 via quench-node `--jit`, `rsscmp.py` (score + max RSS per child), 3 rounds, 2026-10-24.
Baseline `qjitU4` = 10532ead5 (unreachable admitted up to 4096 bytecodes). Candidate `qjitP1` = this change.

Root cause: TSF function 85 (10296 bytecodes) has a 290-way branch table reached by 503 jumps;
`dispatch_path` copied the table at each jump (THREAD_BUDGET counted the table as one instruction),
producing 503 br_tables x 290 edges x 11 block args: 10.6 MB of machine code, 1.35 s SSA sealing
+ 2.2 s Cranelift, 220 MiB peak. sqlite3 function 153: 1.1 s, 72 MiB. Charged by edges: 0.1 s each.

| bench | P1 median | 10532ead5 median | P1 max RSS KiB | base max RSS KiB |
|---|---|---|---|---|
| tsf | 21.27 | 14.02 | 39188 | 20500 |
| sqlite3 | 15.25 | 14.68 | 50560 | 101676 |
| zlib | 26.81 | 25.58 | 20264 | 20492 |
| gcc-loops | 58.63 | 52.61 | 18148 | 17836 |
| quicksort | 88.35 | 85.64 | 13256 | 13260 |
| HashSet | 25.82 | 24.87 | 26100 | 23296 |
| argon2 (1 round) | 0.52 | 0.53 | 17320 | 17316 |

`unreachable-bound-sweep.txt`: the earlier bytecode-bound experiment (4096/8192/16384/unbounded) before the edge charge;
the bound conflicted (TSF needed <=8192, sqlite3 >=16384) because it priced the wrong fact.
