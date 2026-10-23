# Replace through a regexp without keeping a flat copy of a rope

Host: Linux x86_64 VM, 4 vCPUs, kernel 6.18. Binaries: two plain production builds (no BOLT,
dynamic), of v2-gc `2c0cff1989` and of the same tree plus the change (`cand-gc-replace-unkept`).

**Finding:** massif of V8-v7 RegExp under MemoryFirst
(`--threshold=0.0005`, the record spaces' 64 MiB+ reservations left out by `massif_real.py`)
put 842 KiB of the 4362 KiB malloc peak in `JsString::flatten_shared`, 837 KiB of it from
`regexp_symbol_replace_fast`. That fast path matched through an unkept flattening
(`regexp_subject`), but sliced and substituted through `JsString::units`, which flattens and
keeps. Every input rope the benchmark keeps alive then held a flat copy of its own text.

**Change:** the fast path uses `JsString::unkept_flat` (one flattening, which lives only as long
as the operation) in place of the input.

| Measure | Before | After |
| -- | --: | --: |
| massif malloc peak, MemoryFirst | 4362 KiB | 3698 KiB |
| max RSS, MemoryFirst, no JIT (5 runs) | 7812–7964 KiB | 7336–7524 KiB |
| max RSS, MemoryFirst, `--jit` | 8072–8204 | 7528–7612 |
| max RSS, Throughput, no JIT | 13336–13396 | 12756–12808 |
| max RSS, Throughput, `--jit` | 13672–13740 | 13036–13164 |
| Score median, MemoryFirst no JIT / `--jit` | 2122 / 2348 | 2282 / 2219 |
| Score median, Throughput no JIT / `--jit` | 2499 / 2580 | 2614 / 2456 |

The Scores are within this fixture's run-to-run spread (1662–2537 within one configuration).

**Behaviour:** `replace-check.js` (ropes with ASCII and non-ASCII text; `$&`, `` $` ``, `$'`,
numbered and named groups; function replacers; global, sticky and case-insensitive flags;
the legacy `RegExp` statics) prints the same as Node 22 before and after
(`replace-check.node.txt`).
