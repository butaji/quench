# Splay tracking gate on Linux

This clean-source diagnostic evaluates commit `aa88c4a999470b0bb5e655204f3bc1dadaa84457`
on Linux x86_64 (kernel 6.18.44, 4 logical CPUs, 16 GiB memory limit) with
Rust 1.99.0. The candidate production binary SHA-256 is
`1faabc11019a79dfe595440859ea6d059e1e9b18949a0ccc3e3b9ed8d851cc57`.
The matched trunk binary is from `origin/v2` at `883e5c00f`; its SHA-256 is
`e086bf485b76accb09972e0596075aec37dc718c909f183528841a94aeffbf50`.

## Stock Splay

`stock-node26-11.json` records 11 valid, output-equal rounds for Splay. Quench
scores 2,002 against Node `v26.10.0 --jitless` at 5,191. Quench max RSS is
124,289,024 bytes; Bun 1.4.2 without JIT is the lowest-RSS reference at
60,530,688 bytes. This is 38.6% of the Score reference and 2.05x the RSS
reference. Splay is not won.

## Matched fixed-work comparison

`paired-fixed-work-trunk-11.json` compares the exact trunk binary with the
candidate at 300 Splay `run()` calls. All 11 pairs are valid and output-equal.
Median max RSS is 119,652,352 bytes for trunk and 124,190,720 bytes for the
candidate, a 4,538,368-byte regression. Setup-only RSS is 66,097,152 and
66,818,048 bytes, respectively.

Linux `wait4` did not provide cycles or instructions, so the runner reports
0/11 clean rounds and no marginal timing summary. Do not infer a speed win from
this diagnostic. Both reports are one-fixture screens and have
`qualification_ready: false`.

The committed optimization skips young-vector tracking and remembered-bitset
growth when the next scheduled collection is full. It modestly reduces the
previous collector candidate's memory cost, but the clean Splay result remains
below the Score target and above the RSS target. The 1/2 headroom and 2x
full-growth screens were rejected and synced in TOD-10; neither is part of this
commit.
