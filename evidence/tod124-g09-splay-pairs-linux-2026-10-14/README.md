# G-09 Splay screen

Two 11-pair fixed-work screens compared the synced baseline (`6c2771a7f`) with
the G-09 candidate on the pinned V8-v7 Splay fixture (300 `run()` calls per
sample). Both reports are complete and output-equal. The second candidate used
the record mark bit to deduplicate dirty old owners, removing the separate
remembered bitmap.

The host was x86_64 Linux, kernel `6.18.44`, with a 4 CPU cgroup quota and a
16 GiB memory limit. The baseline binary was built from `6c2771a7f` without
`profile-memory`; candidate builds were optimized release binaries. The
fixture corpus was pinned at `64e1860c736c1b899708f4cd646721bc71e53d8b`.

| Candidate | Baseline median work wall | Candidate median work wall | Paired median wall delta | Baseline median max RSS | Candidate median max RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| Initial mark + remembered bitmaps | 1.188 s | 1.192 s | +2.375% | 87,359,488 B | 88,436,736 B |
| Mark bit deduplicates remembered owners | 1.213 s | 1.225 s | −0.521% | 87,371,776 B | 88,064,000 B |

Both screens had 11 valid pairs and 0 clean pairs. The runner therefore
provides no clean-pair wall interval. The compact candidate's median max RSS
was still 0.792% above baseline, exceeding the independent +0.5% RSS budget.
The result rejects this G-09 candidate for adoption; it makes no speed or RSS
improvement claim. The all-eight layout guard was not run because the focused
Splay screen failed the RSS gate. Production collection policy remains
unchanged.

Reproduction from the repository root (with the pinned corpus initialized):

```sh
cargo run --release -p quench-bench --bin quench-bench -- \
  quench-bench/js-engine-benchmark/v8-v7/splay.js --fixed-work --runs 11 \
  --quench target/pinned/baseline/quench-node \
  --quench-peer target/pinned/candidate/quench-node \
  --out target/g09-splay-pair-compact-metadata.json
```

Compressed raw runner reports:

- `initial-bitmap-pair.json.gz` — SHA-256 `d1bbb4709a0381dda7532d544a7baefaa34c3e42c3b9de70504f514780fb4c85`
- `mark-bit-remembered-pair.json.gz` — SHA-256 `8d3e74fb214fc8a7186959a45a9ace659776b810b90d6d74fc4c81d1e4291f12`

The tested baseline executable SHA-256 was `b638f8edb57f54854aedcb4af1b94c6ef087634bc7e2b76ae39c21b3befb6789`.
The compact candidate executable SHA-256 was
`c2404ce14a39a7e7815762fe467f74a485376c1e7a2522202728da7506d19986`.
