# Trimming surviving arrays' empty capacity at full collections: rejected

Host: Linux x86_64 VM, 4 vCPUs, kernel 6.18. Binaries: plain production builds. A is v2-gc
`d676b4a8f3`; B adds `cand-gc-trim-array-slack` (pushed, not merged). `ab.py` (in
`tod137-jit-compile-release-linux-2026-10-22`), 3 runs, `summary.txt`.

**Census** (measurement branch `meas-gc-array-slack`, `profile-memory` build, MemoryFirst):
element capacity against values at each fixture's peak.

| Fixture | Capacity | Values |
| -- | --: | --: |
| DeltaBlue | 119 KiB | 30 KiB |
| Crypto | 490 KiB | 293 KiB |
| RegExp | 291 KiB | 239 KiB |
| others | | nearly equal |

**Candidate:** under MemoryFirst, a full collection reallocates every surviving backing an
array alone owns down to its length (`ArrayElements::trim_slack`).

**Result:** no max RSS gain in either mode. Without JIT, Crypto went from 4796–4800 to
4924–4928 KiB, and DeltaBlue and RegExp stayed within noise. The trimmed tails stay in
malloc's arena and later allocations reuse them, so the process's resident pages and its peak
do not move. Element slack under the peak is malloc-internal, and trimming it needs the
allocator to return pages, which it does not do for small chunks.

**Behaviour:** `array-trim-stress.js` printed the same checksum as Node 22 under both
policies, with and without JIT.
