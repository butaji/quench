# Memory-first pacing restored on trunk (2026-10-18)

Host: `host.txt` (x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB RAM). Comparators: Node v26.12.0 and
Bun 1.4.2 (`BUN_OPTIONS=--smol`).

## What changed

05f1638ed, from the Score lane (`v2-cloud-c`), was merged into v2-merged by e8e40051b. It set:
- the Score lane's GC pacing as the production default: a young collection every eight live
  sets with a 2^15 minimum, a full collection after four live sets with a 2^18 minimum;
- the regex crates' full `perf` features, whose lazy DFAs keep a cache per pattern.

The owner reaffirmed memory-first for trunk on 2026-10-18: the requirement is the lowest peak
RSS among Node and Bun on the npm top 300. Trunk's production pacing is back to the table of
TOD-123, and the regex features are back to `perf-literal`, `perf-onepass` and
`perf-backtrack`.

## Effect on the npm top 300 (`comparison.txt`, release builds, all 300 pass)

| | median Quench MiB | lowest of 300 |
| --- | --- | --- |
| m17: v2-merged 66c7092bf (Score pacing + regex `perf`) | 20.5 | 274 |
| m18: this change | 19.6 | 278 |

| package | m17 | m18 | Bun |
| --- | --- | --- | --- |
| typescript-eslint | 226.4 | 187.9 | 146.2 |
| semver | 45.2 | 27.6 | 50.8 |
| @babel/core | 77.9 | 60.5 | 73.0 |
| color-convert | 37.2 | 23.1 | 44.6 |
| @testing-library/dom | 144.0 | 122.4 | 120.7 |

A `gc-tuning` build of m17 with the memory-first pacing set through `QUENCH_GC_*` attributes
the rise:

| package | m17 | memory-first pacing only | m18 |
| --- | --- | --- | --- |
| semver | 45.2 | 32.5 | 27.6 |
| @babel/core | 77.8 | 64.0 | 60.3 |
| typescript-eslint | 226.1 | 194.3 | 187.9 |

Pacing accounts for most of it; the regex lazy DFAs add up to 5 MiB more. `rss-report.txt`
and `npm-scenarios.json.gz` are m18. The Score lane's own measurements of its table are in
05f1638ed (Splay 4566 → 5931, RegExp 931 → 1042; Splay max RSS 40 → 162 MB).
