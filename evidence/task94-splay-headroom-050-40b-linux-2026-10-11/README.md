# Splay collector headroom screens on 40B cells

Date: 2026-10-11  
Branch: `v2-cloud`  
Benchmark source revision: `384a16e35b693eac82bf6b9b71add9dbe9c6773f`  
Runtime toolchain: Rust 1.99

These are Linux fixed-work and stock Splay screens on the Task61 compact-cell candidate integrated with the current `v2-cloud` collector. The 0.5x candidate changes only the large-heap allocation headroom factor from 1/1 to 1/2; full-collection growth remains 1/1 and first-survivor cells promote immediately. It is an isolated experiment. No collector policy change is retained.

## 0.5x immediate promotion

The stock benchmark report is valid and output-equal in all 11 rounds, but not qualification-ready because Linux `perf` counters are unavailable. The same-host 1/1 control was run with the same harness and engines.

| Engine | 1/1 control Score | 0.5x Score | 1/1 max RSS | 0.5x max RSS |
|---|---:|---:|---:|---:|
| Quench | 2,026 | 1,743 | 99,295,232 B | 98,701,312 B |
| Bun | 2,017 | 1,801 | 61,755,392 B | 61,751,296 B |
| Node | 2,541 | 2,160 | 403,656,704 B | 384,122,880 B |
| QuickJS | 1,690 | 1,907 | 149,553,152 B | 149,618,688 B |

Against the 1/1 Quench control, 0.5x loses 13.96% Score and saves 593,920 B. Quench remains 36,950,016 B above Bun's RSS and its Score remains below Bun's. This does not meet either Splay win bar.

The fixed-work diagnostic matched output in 11/11 rounds. Median work RSS was 98,770,944 B versus 99,270,656 B for the 1/1 control; setup-only RSS rose from 54,149,120 B to 54,996,992 B. Since `perf` was unavailable, fixed-work timing is not used as a qualification claim.

## Two-survival promotion screen

A separate experiment added an intermediate generation and promoted cells after their second survival, retaining the 0.25x headroom factor. The full-mark debug verifier completed a 300-iteration Splay profile without a mismatch. Runtime tests passed 552/553; the one failure was the environment-dependent German date-format test producing `2.1.1970` instead of `1.1.1970`.

Fixed-work output matched 11/11, but median work RSS rose to 97,529,856 B from 96,636,928 B for immediate promotion (+892,928 B); setup RSS also rose by 528,384 B. The intermediate-generation code is not retained.

## Files

- `task94-gc-40b-headroom-050-stock-11.json`: 0.5x stock Splay against Node, Bun, and QuickJS.
- `task94-gc-40b-full-1x-stock-11.json`: same-host 1/1 control.
- `task94-gc-40b-headroom-050-vs-full-1x-splay-fixed-11.json`: fixed-work 0.5x versus 1/1 comparison.
- `task94-gc-40b-2survival-025-splay-fixed-11-v2.json`: two-survival versus immediate-promotion fixed-work comparison.
- `headroom-050.patch`: isolated threshold and unit-expectation change used for the 0.5x screen.

## Research reference

V8's GC documentation describes young-generation collection using a remembered set for old-to-young references and major collection of the old generation: <https://chromium.googlesource.com/v8/v8/%2Bshow/lkgr/docs/heap/garbage-collection.md>. This informed the two-survival screening hypothesis; the measurements above reject that particular implementation for Splay.
