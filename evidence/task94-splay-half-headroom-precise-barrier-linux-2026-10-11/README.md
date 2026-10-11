# Splay: 1/2 headroom with precise property barrier

Date: 2026-10-11  
Source: `3074bf1b3e74f64450aace493c73fab21cf64982`  
Rust: `rustc 1.99.0 (b940084d7 2026-09-28)`  
Host: Linux x86_64, kernel `6.18.44`, 4 logical CPUs, 16 GiB memory limit  
Binary SHA-256: `50a44964c8c423b06ec8543c511dc74925f4f49b52160b7af5048d430e6daea2`

## Result

The stock V8-v7 Splay run used 11 rounds. Every engine produced valid output,
and Quench matched the other engines' observable result. The raw report is
[`stock-splay-11.json`](stock-splay-11.json); it records `source_dirty: false`
and `qualification_ready: false` because this is one fixture only.

| Engine | Median Score | Median max RSS |
| --- | ---: | ---: |
| Quench | 1,957 | 122,777,600 B |
| Node v26.10.0 `--jitless` | 5,232 | 402,149,376 B |
| Bun 1.4.2, JIT disabled and probed | 2,958 | 60,071,936 B |
| QuickJS 2026-06-04 | 2,706 | 149,622,784 B |

Quench reached 37.4% of Node's Score and used 2.04x Bun's max RSS. It missed
both Splay bars. Node's RSS is reported for completeness; the Splay RSS bar is
Bun with JIT disabled.

The preceding same-host 1/1 headroom control scored 2,094 with Quench max RSS
of 123,875,328 B. The candidate was 137 Score points lower and used 1,097,728
fewer bytes. These were sequential screens, not paired runs, so the difference
is directional and does not establish a causal gain from the headroom change.
The control is preserved in the sibling evidence directory
`task94-splay-full-growth-5-4-linux-2026-10-10/stock-splay-1x-control-11.json`;
its report has `source_dirty: true`, although the measured executable was the
clean `aa88c4a99` build.

## Candidate and decision

The candidate combined large-heap headroom 1/2 with `property_set` remembering
only old-owner to young-value edges. Full-collection growth remained 1/1. The
focused remembered-edge and collection tests passed before the stock run.

The candidate does not meet the joint Score/RSS target. For the next screen,
large-heap headroom is restored to 1/1 while the precise `property_set`
barrier remains in place. This isolates that barrier's effect against the
existing policy. The policy value is a proposal for the trunk owner to review;
the lane does not own the trunk's production GC policy.

## Reproduction

```sh
source /workspace/quench-build-env.sh
PATH=/workspace/quench/target/stageb-tools/node-v26.10.0/bin:/workspace/quench/target/stageb-tools/bun/bun-linux-x64:/workspace/quench/target/stageb-tools/qjs/quickjs-2026-06-04:$PATH \
target/release/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench /workspace/quench/target/splay-125/production/quench-node \
  --qjs /workspace/quench/target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --node /workspace/quench/target/stageb-tools/node-v26.10.0/bin/node \
  --bun /workspace/quench/target/stageb-tools/bun/bun-linux-x64/bun \
  --runs 11 --timeout-ms 300000 \
  --out /workspace/quench/work/splay-precise-half-headroom-stock-11.json
```

## Research note

The remembered-set design follows the measured tradeoff described by Detlefs
et al.: more precise remembered sets reduce young-collection scanning, at the
cost of work on the write barrier. This candidate screens that tradeoff on the
actual Splay workload rather than assuming the precision wins:
[USENIX JVM '02 paper](https://www.usenix.org/publications/library/proceedings/jvm02/full_papers/detlefs/detlefs_html/index.html).
