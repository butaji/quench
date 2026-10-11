# Splay shared-prototype trace filter screen

Date: 2026-10-11 UTC  
Branch: `v2-cloud`  
Base revision: `7cbdfa504`  
Rust: `1.99.0 (b940084d7 2026-09-28)`

## Candidate

Splay objects often share a prototype. The candidate moved prototype enqueueing after the object's other outgoing edges and checked the mark bit before enqueueing a heap prototype. This aimed to reduce duplicate prototype worklist entries while preserving the prototype edge. The exact unretained source diff is in `shared-prototype-filter.patch`.

Candidate production binary SHA-256: `f1f1cc02cecc6b0972b0979977f3ba1435c5e91665c02a69a8d2719c8871917d`. Generic control production binary SHA-256: `6ee6c8364de11bd52cd37297c757d08a43dda16cc929dc0c4ff69c3ecb516d41`.

## Correctness and instrumentation

The added heap test passed 1/1; the Rust 1.99 heap suite passed 27/27; runtime library tests passed 548/548; Test262 FinalizationRegistry passed 47/47, WeakMap/WeakRef/WeakSet passed 255/255, and the full Wasm directive suite passed 67,124/67,124.

The Linux host has no `perf`; all 33 fixed-work pairs were valid and output-equal but 0/33 were counter-qualified. An attempt to build the aggregate-profile feature was blocked by an existing out-of-lane compile error at `crates/quench-runtime/src/vm/dispatch.rs:800`: `regional_binary` is called with three arguments while its definition requires four. That file was left unchanged.

## Results

Across three fixed-work batches (11 pairs each), pooled setup-adjusted marginal wall time had a median delta of **−6.73%** (candidate faster), with bootstrap 95% interval **[−10.92%, +5.40%]** and **20/33** candidate wins. The paired RSS median delta was **−57,344 B** (20/33 candidate wins). The interval includes no change and the host could not qualify the timing rounds.

The 11-round stock candidate run was valid/output-equal: Quench Score **1,841** vs Node `--jitless` **4,221**; Quench max RSS **124,014,592 B** vs Bun no-JIT **60,403,712 B**. A fresh generic control run scored Quench **2,263** vs Node **5,615** and used **124,022,784 B** vs Bun **60,276,736 B**. Sequential host scores drifted substantially; these unpaired stock values do not establish a speed effect. The candidate still misses both bars by a wide margin.

**Decision:** reject and revert the source change. The paired timing interval includes no change, the RSS difference is negligible, and the stock candidate misses the Score and RSS bars. Splay remains active and unwon.

## Reports and hashes

- `fixed-11.json` SHA-256: `779da5283f65f428d2627eef2ce222ae584ce522b78852be428d27a1c08d8650`
- `fixed-repeat-11.json` SHA-256: `04b9c61379076f288828fd923501e039414e1be74a2d5e4934a85b4c9cd3eeb9`
- `fixed-third-11.json` SHA-256: `8e0c68fcf41855a777ef4c0289a08520df446ebb601dd03736e7fd7864fbb13d`
- `stock-candidate-11.json` SHA-256: `2d5601b455df8b2c7a02f51db9307adef4e4be34426b71788a0a70289d307fea`
- `stock-control-11.json` SHA-256: `5e971ad96a74fab9c76404340226ee5e0f4f80a2f90606eceac12b8bdc53f486`

## Reproduction

Build candidate and control binaries with Rust 1.99 under production profile, then run the same fixed-work Splay fixture with 11 paired rounds for each batch. The raw reports preserve runner arguments, source and binary identifiers, per-round wall time/RSS, output checks, and setup samples. The candidate source diff is retained here for review, but no candidate code is retained in the branch.
