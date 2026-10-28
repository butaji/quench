# Release pages around JavaScript native compiles

Host: Linux x86_64 VM, 4 vCPUs, kernel 6.18. Binaries: plain production builds (no BOLT,
dynamic). A is v2-gc `930ef31090`; B is the same tree plus `cand-gc-jit-compile-release`.
`ab.py` runs both, interleaved, 3 runs, evicting each binary first. `summary.txt` gives
max RSS ranges in KiB and Score medians.

**Why:** with `--jit` the peak of a short numeric program is a compile. The code generator's
code pages and scratch join the interpreter's resident code and the heap. massif put 4.3 MB of
compiler malloc at NavierStokes's peak (TOD-110 comment of 2026-10-22).

**Change:** `Host::before_wasm_native_compilation` becomes `before_native_compilation` and
now also runs before a JavaScript function or OSR region compile. It releases read-only
pages, as before WebAssembly compiles. The new `after_native_compilation` releases read-only
pages and the allocator's free pages once a JavaScript compile is done. Both run under either
memory policy: compiles are rare and bounded per site.

**Max RSS, `--jit`** (KiB, min–max of 3):

| Fixture | MemoryFirst A | MemoryFirst B | Throughput A | Throughput B |
| -- | --: | --: | --: | --: |
| richards | 4440–4504 | 4144–4396 | 5388–6028 | 5052–5308 |
| deltablue | 5496–5500 | 4420–4540 | 9104–9240 | 7792 |
| crypto | 6836–6840 | 4988–5276 | 11636–13172 | 9232–9660 |
| raytrace | 4368–4760 | 4396 | 8144 | 7452–7456 |
| earley-boyer | 8088–8308 | 8116–8184 | 12440–12580 | 11768–12296 |
| regexp | 7336–7576 | 7540–7564 | 13032–13048 | 12260–12296 |
| splay | 32488–32560 | 32476–32492 | 34420–34452 | 33560 |
| navier-stokes | 10772–10976 | 8696–8904 | 11112–11240 | 9052–9124 |

**Scores:** medians are within the fixtures' run-to-run spread; the largest moves are
Throughput RayTrace 2258 → 2574 and MemoryFirst RegExp 2364 → 2155, in opposite directions.
**No JIT:** unchanged, as expected; it does not compile.
