# Splay empty-slot full-sweep screen

Date: 2026-10-11 UTC  
Branch: `v2-cloud`  
Base revision: `65bb441da0e81bfdf1bd56fbc61fa238af1c628a`  
Rust: `1.99.0 (b940084d7 2026-09-28)`

## Candidate

The full sweep checks the mark bit for every slot and calls `Option::take` on unmarked slots. After the heap has reached its high-water capacity, many slots are already empty. The candidate skips those slots before checking the mark bit. No threshold or marking behavior changes. The exact source diff is in `empty-slot-sweep.patch`.

The candidate binary SHA-256 is `017144e235750f6658dc1adffc7c983822a70e5fff2805e30da58aee1ca87ab1`. The generic-barrier 1/1 control binary SHA-256 is `6ee6c8364de11bd52cd37297c757d08a43dda16cc929dc0c4ff69c3ecb516d41`.

## Correctness

Rust 1.99 heap unit suite passed 26/26. Both fixed-work Splay batches and both stock Splay runs were valid and output-equal in all 11 rounds.

## Results

The fixed-work reports are diagnostic: this Linux host had no `perf` counters, so 0/22 rounds were counter-qualified. Pooling both batches after subtracting each K=0 setup sample, the paired marginal wall-time median was −6.50% (candidate faster), with a bootstrap 95% interval of −12.16% to +3.27%; 15/22 pairs favored the candidate. Paired median max RSS was unchanged (0 B; candidate won 10/22 pairs). The interval includes no change.

The candidate's 11-round stock report was valid/output-equal: Quench Score 2,259 vs Node `--jitless` 5,527; Quench max RSS 123,887,616 B vs Bun no-JIT 60,141,568 B (2.06×). A fresh control stock run scored 1,833 vs Node 3,307 and used 123,826,176 B vs Bun 60,493,824 B. The large change in reference-engine Scores between sequential runs shows host drift; these stock candidate/control values do not establish a Score regression or gain. Both candidate bars remain missed.

**Decision:** reject the source change. The paired screen does not confirm a timing win and shows no RSS improvement; the stock candidate still misses both Splay bars. No code change is retained.

## Reports and hashes

- `splay-empty-sweep-fixed-11.json` SHA-256: `5726c906b21def1b7613127fc0c6e8d9243bc74501144de5953ca900491123fe`
- `splay-empty-sweep-fixed-repeat-11.json` SHA-256: `bcbad683b64bbc5f01c6a264b635e22824c06b6805de3c45ecdbf2d2e3f6d234`
- `splay-empty-sweep-stock-11.json` SHA-256: `798ab800df3a5f7ba2212f6026424076e9664c29ce2d98d4a479376f4604971e`
- `splay-empty-sweep-control-stock-11.json` SHA-256: `3ce198f12a5cdaa15cb05f040b417dddd0aff7517b25c9a602d6d3c65193824a`

All reports identify source revision `65bb441da0e81bfdf1bd56fbc61fa238af1c628a`; the experiment source was dirty while the candidate was built and measured.

## Reproduction

With the candidate patch applied to the base revision, build under Rust 1.99:

```sh
CARGO_TARGET_DIR=target/splay-125 cargo build --locked --offline --profile production -p quench-node --bin quench-node
```

The paired diagnostic command was:

```sh
target/release/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench target/splay-control/production/quench-node \
  --quench-peer target/splay-125/production/quench-node \
  --runs 11 --timeout-ms 300000 --out work/splay-empty-sweep-fixed-11.json
```

The repeat used the same command and binaries, writing to `work/splay-empty-sweep-fixed-11-repeat.json`.

The stock candidate and control reports used the same fixture, engine paths, 11 rounds, and 300,000 ms timeout, with the corresponding Quench binary passed to `--quench`.
