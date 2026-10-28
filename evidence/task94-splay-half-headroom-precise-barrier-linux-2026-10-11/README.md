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

## 1/1 precise-barrier follow-up

At commit `bb8c6b87eb16e4b48d7b471c3b49cf3203976e09`, large-heap headroom
was restored to 1/1 while the precise `property_set` barrier remained. This
isolated stock run used 11 rounds and is in
[`stock-splay-precise-barrier-1x-11.json`](stock-splay-precise-barrier-1x-11.json).
It was valid and output-equal, but its Score was 1,593 vs Node `--jitless`
2,852, and max RSS was 123,887,616 B vs Bun no-JIT 60,530,688 B. This also
missed both bars.

Because the references shifted from the previous session, I built a clean
generic-barrier control from `0c9831f8dc7b23393a3d0fc59063f6ae46522998` and
alternated 11 one-round control/candidate pairs, reversing order on every
other pair. Each report was valid and output-equal. The control and candidate
production binary SHA-256 values were respectively
`6ee6c8364de11bd52cd37297c757d08a43dda16cc929dc0c4ff69c3ecb516d41` and
`accda9aeaeb2c38d6573140e07cc9820209e873204f7becea0d1a41f8ce60032`.
Paired measurements and the deterministic 20,000-resample bootstrap summary
are in [`paired-summary.json`](paired-summary.json); raw one-round reports
remain in the ignored `work/` directory.

| Paired measure | Generic barrier | Precise barrier | Paired delta (95% bootstrap interval) |
| --- | ---: | ---: | ---: |
| Median Quench Score | 2,060 | 2,166 | +2.48% (-3.40%, +9.43%) |
| Median Quench max RSS | 123,957,248 B | 123,949,056 B | -69,632 B (-200,704 B, +61,440 B) |

The candidate won Score in 7/11 pairs and RSS in 6/11. The Score and RSS
intervals both include no change. The paired Quench/Node Score ratio also had
a 95% interval spanning zero. This does not establish a performance or memory
benefit, so the precise barrier is rejected and its source change is removed.

## Decision

The 1/2 candidate combined reduced headroom with `property_set` remembering
only old-owner to young-value edges. Full-collection growth remained 1/1. The
focused remembered-edge and collection tests passed before the stock run.

The 1/2 candidate loses Score and saves only about 1.1 MB against the
sequential 1/1 control. The precise barrier at 1/1 has no measurable paired
benefit. The production barrier therefore returns to the generic behavior and
large-heap headroom stays at 1/1. The trunk retains ownership of the named GC
policy.

## Reproduction

Use the same stock command shown below for the original `3074bf1b3` candidate
and the 11-round `bb8c6b87e` follow-up, changing the Quench executable to the
corresponding recorded binary. The paired screen ran the command with
`--runs 1` once for each binary in every pair, alternating which ran first.
It used the exact engine binaries recorded in the JSON reports. The control
source is `0c9831f8d`; the candidate source is `bb8c6b87e`.

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
