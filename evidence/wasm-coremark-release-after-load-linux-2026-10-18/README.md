# Read-only pages released after host loads and Wasm compilation (Linux x86_64, 2026-10-18)

Intel Xeon @ 2.10 GHz, 4 vCPUs, 15 GiB, Linux 6.18.44, no cgroup limits. Static production
`quench-node`; base `v2-merged` `48027f608`, candidate the same tree with this change.

## What set the peak

CoreMark's max RSS without the JIT was a transient of file-backed pages, not live data:
sampling `/proc/self/status` every millisecond put the peak at about 47 ms (2.5 MB anonymous,
5.7 MB file), while the run itself holds 2.7 MB anonymous and 3.5 MB file. The pages were the
binary's code and read-only data touched while the host builds `fs` (its JavaScript
snippets) and while the module decodes, validates and lowers. Both run once, after the
existing release points at bootstrap and after the entry compiles.

## Change

- `NodeHost::compile_wasm` releases the read-only pages (`release_read_only_pages`) once the
  module has lowered, before it runs.
- A builtin the host builds is a host load like a library module: `host_load` (formerly the
  library-only `LIBRARY_LOADS` depth) counts nested loads, a build or a library compile marks
  it, and the outermost load releases once when it ends. Releasing after every nested build
  instead cost 1.6–3.2% of a 20-builtin start-up; once per outermost load costs nothing
  measurable.

## Results

Max RSS (3 runs each, all identical within 16 KB):

| Run | Base | Candidate |
| --- | ---: | ---: |
| CoreMark harness, no JIT | 7,600 KB | 6,200 KB |
| CoreMark harness, `--jit` | 8,508 KB | 7,728–7,740 KB |
| empty script | 3,912 KB | 3,920 KB |
| `import fs from 'node:fs'` | 6,456 KB | 6,204 KB |
| `require('fs')` | 5,684 KB | 5,552 KB |
| 20 builtins required | 19,096 KB | 18,960 KB |

CoreMark harness: `evidence/wasm-coremark-quench-node-linux-2026-10-17/run-js.mjs` on
`coremark-minimal-mvp.wasm`.

Cost, `perf stat -r 20` task clock, three interleaved rounds (base / candidate):

| Script | Round 1 | Round 2 | Round 3 | Page faults |
| --- | --- | --- | --- | --- |
| empty | 18.0 / 16.2 ms | 19.3 / 18.5 ms | 17.7 / 17.6 ms | 5,588 / 5,636 |
| 20 builtins | 566.9 / 567.7 ms | 564.1 / 577.8 ms | 594.2 / 593.6 ms | 94,994 / 96,840 |

JetStream Wasm fixtures with `--jit`, one iteration (compile included) through
`evidence/wasm-jit-loops-and-addresses-linux-2026-10-18/jetstream-fixed.cjs`: page faults
+0.4% (gcc-loops) to +4.4% (zlib); TSF task clock in four interleaved rounds of five runs
1,349/1,338, 1,356/1,353, 1,375/1,370 and 1,395/1,365 ms.

## Checks

`child_entry` (481 cases, Node v26.12.0 on `PATH`): base 443 passed, 38 failed; candidate 444
passed, 37 failed. Three outcomes differ between the runs, all timing:
`global_crypto_is_installed_before_commonjs_entry_and_preloads` finishes at its 5-second
child deadline in the debug build on either tree (9.4–9.7 s base, 9.6–10.1 s candidate for its
four children; it timed out once in three runs on the candidate alone), and
`assertion_exception_objects_share_strict_property_comparison` and
`child_stdin_backpressure_preserves_bytes_and_finishes_synchronously_drained_tail` failed on
base only. The production build loads `crypto` in the same time (10-run means 388 and 387 ms
over three interleaved rounds) with 3% fewer page faults.

## Standing against the 2026-10-17 campaign

`--jit` 7.7 MB is below Winch (12.00 MB) and Wasmtime (14.07 MB). Without the JIT, 6.2 MB
remains above Wasm3 (4.04 MB): the empty process is 3.9 MB.
