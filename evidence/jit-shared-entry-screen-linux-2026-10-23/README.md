# Shared entry and adapter bypass screen — 2026-10-23

This is a three-round JetStream 3 `hash-map` screen on the available Linux x86_64 host. It is not a qualification result: only one of 30 fixtures was run, and three rounds do not meet the 11-round acceptance minimum.

The baseline used merged source revision `97eac7b8f`, production Quench binary SHA-256 `5c03bd54ddfba0bab2ee4a295faf26f8394387c904190ae8d83b6331b8ccc0de`, Node v24.19.0, and Bun 1.4.2. Median scores / peak RSS were Quench 14.61 / 39.6 MB, Bun 247.32 / 102.4 MB, and Node 159.35 / 145.3 MB.

The diagnostic-only report is a separate one-round run with `QUENCH_JS_JIT_STATS=1`; its timings are not comparable to the baseline. It recorded 53,999,969 native entries into `equals` and an interpreted `runIteration` with `LoadNameCall` and `Call`, with 43.4 seconds accumulated interpreter time.

The adapter-bypass experiment was built from the same source revision plus an uncommitted code change (report `source_dirty=true`), production binary SHA-256 `29372d4ed7cf1ec4c543400e0f980e1423d57e9698ae8706a384974f9e5b5f65`. Its three-round Quench median was 13.31 / 39.6 MB. Per-round scores overlap heavily with baseline and show no benefit; the experiment was reverted. Its report is retained to document that decision.

The baseline acceptance gate remains unmet. Continue with the shared `compiled.rs` region compiler, exact PC exits, and general JS call/IC lowering. The compiled caller and native entry measurements point to JS call coverage as the next important path; this screen does not establish a speedup from the native leaf functions alone.

## Compiled-artifact borrow screen

A follow-up candidate borrows the cached JS tier artifact instead of cloning its `Rc` on each compiled call. It passed the 841-test runtime library suite. The three-round production screen is in `hash-map-no-rc-clone.json`, using Quench binary SHA-256 `56e5a5c6f5fd96971e5a666bde1678c68b6a0e06c7f2006a932485e2a53484ea`. Median scores / peak RSS were Quench 15.21 / 39.65 MB, Bun 259.38 / 113.04 MB, Node 185.35 / 231.06 MB.

Compared with the earlier three-round baseline report, Quench's per-round scores changed from 10.08, 14.61, 14.92 to 10.77, 15.21, 15.35. These were separate sequential screens, not paired rounds; the consistent positive direction is a screen only. Quench still trails both comparators by a wide margin, and this result does not qualify TOD-110 or TOD-138.
