# Splay empty-page return screen

Date: 2026-10-11 UTC

Branch base: `1743f0c2cb0010c0ca3ccc5efb05ca62fd35fc53`

Rust: `1.99.0 (b940084d7 2026-09-28)`

Host: Linux x86_64, kernel `6.18.44`, 4-CPU quota, 16 GiB memory limit,
4,096-byte pages

## Question

The prior profile-only census found 4,201 fully empty pages (17,207,296 B)
after Splay GC18. This screen asks whether returning these pages to Linux lowers
the stock maximum-RSS peak without reducing Score.

## Candidate and correctness

The candidate wraps each slot's `Option<Cell>` in `MaybeUninit`, tracks
decommitted pages, skips slots that touch them, and writes `None` before a slot
is reused. It marked pages touched by reclaimed slots and screened pages after
both full and minor sweeps. The focused full/minor page-return and reuse tests
passed 2/2. Rust 1.99 warnings-denied checks passed for `quench-node` with and
without `profile-memory`.

The compressed source patch is `page-return-candidate.patch.gz` (SHA-256
`f22fcbc4e147eb65739613d8ef8e4c6feaf4b2c5829089d50dfc214cda74caee`; its
uncompressed SHA-256 is
`950245104c3cfb43f11728ae0e520d33fb441f40a849b9e14e75560d45ad174c`). The
candidate production binary SHA-256 is
`d1757b79db0252c72f63d5a8904db05102a7ed73aae2fba2e67048506b01213c`.
The clean branch-head control binary SHA-256 is
`5c821038a08b3062f210e53d06da3d26bb2bb4a30a53ca20510fa4070a7ee042`.

## Results

All three Splay reports were output-equal. The first full-sweep-only candidate
had 11/11 valid stock rounds: Quench median Score 1,314 and max RSS
116,654,080 B, versus Node `--jitless` Score 4,254 and Bun no-JIT Score 3,520 /
max RSS 62,148,608 B. Its report is not qualification-ready.

The lower-overhead candidate also had 11/11 valid stock rounds: Quench median
Score 1,469 and max RSS 116,936,704 B, versus Node `--jitless` Score 4,898 and
Bun no-JIT Score 3,562 / max RSS 61,620,224 B. The engine scores drifted
sharply across sequential rounds, so these stock numbers are only a rejection
screen.

An exact branch-head fixed-work pair compared the clean control binary and
candidate binary in the same 11-round run. Outputs matched; all rounds were
valid, but 0/11 were counter-qualified because Linux `perf` counters were
unavailable to the runner. Median work max RSS was 116,903,936 B for control
and 117,014,528 B for candidate (+110,592 B). Setup-only max RSS was
63,676,416 B and 63,946,752 B respectively (+270,336 B). There is no measured
timing win and the RSS peak did not fall.

**Decision: reject the page-return source change.** The page census reports
potentially reclaimable bytes after collection, while the benchmark's maximum
RSS records the higher pre-sweep peak. This collector-only path cannot meet the
Splay bars on this evidence. The current Splay bar definition is Score above
Node `--jitless` and max RSS below Bun no-JIT; Linux requires its own clean
qualification run. See Linear TOD-7 for the current cross-host gates.

## Research notes

The Linux `madvise(2)` manual specifies page-aligned whole-page ranges for
`madvise` and describes the destructive behavior of `MADV_DONTNEED` on private
anonymous mappings. The candidate therefore does not treat discarded bytes as
a valid `Option<Cell>`: `MaybeUninit` makes arbitrary page contents safe to
hold, accesses skip discarded slots, and restoration initializes them to
`None` before use. See the [Linux man-pages `madvise(2)` reference](https://man7.org/linux/man-pages/man2/madvise.2.html)
and the [Rust `MaybeUninit` safety and validity documentation](https://doc.rust-lang.org/std/mem/union.MaybeUninit.html).

## Reports and hashes

- Full-sweep-only stock JSON, compressed; SHA-256
  `9220d9de217350a57e7687c183fe128ef9a2405cb2635522891d0d5ca85fe538`.
- Candidate-pages stock JSON, compressed; SHA-256
  `f794e18eba98287b90b263f31c497477f9c26943b6c8b4f25f918c4714a47cee`.
- Candidate fixed-work JSON, compressed; SHA-256
  `594f471df69618e05b55ee42634ab881584ed76ed66bb4230491014846d1bf3e`.
- Raw runner inputs, binaries, and tool versions are recorded in each report.

## Reproduction

Decompress and apply `page-return-candidate.patch.gz` to the branch base, then
build with Rust 1.99:

```sh
gzip -dc evidence/task94-splay-page-return-linux-2026-10-11/page-return-candidate.patch.gz | git apply
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' cargo +1.99.0 build --locked --offline \
  --profile production -p quench-node --bin quench-node \
  -p quench-bench --bin quench-bench
```

Stock screen:

```sh
target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench target/production/quench-node \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 --out work/splay-page-return-stock-11.json
```

The fixed-work pair used the clean branch-head and candidate binaries with
`--fixed-work`, `--runs 11`, and the same Splay fixture. The raw JSON records
both exact binary hashes and the missing performance-counter qualification.
