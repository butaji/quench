# MemoryFirst first-call release by code growth: rejected

**Candidate** (`cand-gc-growth-release2`): under MemoryFirst, a first call releases the
read-only pages once the resident file bytes have grown by 256 KiB since the last release. The
time budget is kept, the 5 ms interval is dropped, and so is the 512 KiB absolute budget.

**Arms:**
- The old rule (5 ms interval plus 512 KiB budget) runs in the same binary behind `QX_OLD`, a
  temporary switch that was never committed.
- `grow_nocap` drops the time budget as well.

**Host:** 4-vCPU Intel Xeon @ 2.80GHz x86_64 VM, MemoryFirst, binaries evicted.

| Run | Binary | Result |
| -- | -- | -- |
| `unbolted-9-runs.txt` | unbolted static | Crypto worst −72 to −204 KiB; NavierStokes `--jit` median +124; RayTrace worst +100. `grow_nocap`: npm CPU 1.4–2.2×. |
| `hybrid-9-runs-loaded-host.txt` | growth or 5 ms, whichever first | Concurrent builds loaded the host, so only the arms' relative order holds: no gain over growth alone. |
| `bolted-cdsort-15-runs.txt` | current v2-gc, bolted `cdsort`, 15 interleaved runs | RayTrace `--jit` worst +228 KiB; RegExp median −76 (worst equal); DeltaBlue `--jit` median +116; Richards median +112 (worst −76). npm within noise. |

**Decision:** no row improves consistently, and the bolted re-run raises one worst row. The
rule stays time-based. Rejected.
