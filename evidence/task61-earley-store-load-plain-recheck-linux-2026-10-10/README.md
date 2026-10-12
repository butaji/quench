# EarleyBoyer plain-local store/load recheck

## Retained memory optimization

The V2-merge profile recorded 54,861,036 adjacent `StoreLocalPlain → LoadLocalPlain` executions on EarleyBoyer. The profile and census are in [the V2 merge record](../task61-earley-v2-merge-linux-2026-10-10/README.md). I remeasured this pair against the later `v2-cloud` baseline after earlier EarleyBoyer optimizations had changed the execution mix.

The compiler now folds a same-slot plain-local load into the preceding plain-local store by setting the store's existing optional output register. It declines pairs when that output is already in use, the slots differ, or the load has a numeric-update side effect. The runtime already performs this optional register write after the local write, preserving the original order.

Node v24.19.0 matched both Quench binaries on object identity through a local store/read, plus own and inherited getters, nested property reads, `undefined`, and a throwing getter with identity preservation. The focused rewrite test passed. EarleyBoyer output matched in all 11 alternating production pairs.

Against baseline SHA-256 `88e7706b6e85bc036dfe5fe7a020e86e86ae3e9d8789fb506b2aea6c3fa6590a`:

- Score medians: 408 baseline, 405 candidate. Paired median delta −3 points; 95% bootstrap interval [−8, +2], so the data includes a tie and does not show a Score win.
- Median maximum RSS: 42,721,280 baseline, 42,242,048 candidate. Paired median delta −536,576 bytes; 95% interval [−663,552, −393,216]. Candidate RSS was lower in all 11 pairs.
- Candidate binary SHA-256: `3b305f12f60a7eb6024b305204a3bc07aea965f34bbeeda8e1885f02df61c32b`.
- Fixture SHA-256: `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

This is a Quench-only RSS improvement for cumulative Earley tuning. It is not a Score win against the comparison engines and does not qualify Stage B.

The earlier three-pair rejection [recorded here](../task61-earley-plain-local-fusion-linux-2026-10-10/README.md) used the pre-merge baseline at SHA `f3940c…`. Its negative result remains valid for that baseline. This recheck uses the later baseline at `88e770…`, a longer sample, and exposes the RSS benefit after the subsequent Earley changes; it does not erase the earlier result.

## Rejected field-get variants

The same profile showed 23,588,682 adjacent `GetField → StoreLocalPlain` executions. I tried two implementations because the first added an opcode and dispatch arm; the second reused `GetField` result flags. Both matched Node and fixture output and both lowered RSS, but neither improved Score:

- Fused opcode: 11 pairs, Score median delta −3 (95% interval [−8, −1]); RSS median delta −585,728 bytes (95% interval [−724,992, −319,488]). Rejected for a repeatable Score loss.
- Existing-op result flag: three-pair screen, Score deltas −3, −7, −3; RSS median delta −684,032 bytes. Rejected at the directional screen.

The candidate patches, reports, runners and raw stdout/stderr are under [rejected-field-opcode](rejected-field-opcode/) and [rejected-field-result-flag](rejected-field-result-flag/).

## Method and host

The pinned V8-v7 EarleyBoyer fixture was materialized by `work/task61-direct-number-screen/runner.py`. Production binaries used `cargo build -p quench-node --profile production`. Pairs alternated baseline-first and candidate-first. The 95% intervals resample the paired deltas' medians with 100,000 bootstrap draws and fixed seeds. The complete pair records and runner are in this directory; `raw/` preserves each measured stdout/stderr.

Host limits and tool versions are in [host.txt](host.txt). `origin/v2` was fetched before this work; `FETCH_HEAD` was ancestor `dfa08983`, so there were no newer v2 changes to merge. The paired result here uses the current `v2-cloud` code and does not import the older field-get experiments.

## Research note

The profile-guided direction is consistent with the superinstruction literature: common adjacent VM operations can benefit from reducing dispatches, but the empirical result is VM- and workload-specific. Casey, Gregg, Ertl and Nisbet describe this approach and its measured Java interpreter speedups in [Towards Superinstructions for Java Interpreters](https://doi.org/10.1007/978-3-540-39920-9_23). That paper motivates measuring hot pairs; the paired EarleyBoyer results above decide whether each Quench variant stays.
