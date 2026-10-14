# Splay on Linux after trunk integration

## Setup

Measured on Linux x86_64, kernel 6.18.44, 4 vCPU, 16 GiB cgroup memory limit,
with Rust 1.99.0. The source was clean at `7e47868cc5dc2ba8236716a5a45865812858bae3`
and the V8-v7 submodule was clean at `64e1860c736c1b899708f4cd646721bc71e53d8b`.
The candidate production binary SHA-256 is
`ad7c378969c0072509463752b7f7597b9ee4a2c5b51160420f2c1f1cc2a60ead`.
Reference engines were QuickJS 2026-06-04, Bun 1.4.2 with JIT disabled, and Node
v26.10.0 with `--jitless`.

The trunk comparison binary was built from clean `origin/v2` at `883e5c00f`
with the same production profile and toolchain; its SHA-256 is
`e086bf485b76accb09972e0596075aec37dc718c909f183528841a94aeffbf50`.

## Stock-harness result

The candidate Splay fixture is valid for 11/11 rounds and output matched across
all engines. Median Quench Score is 2,084; Node `--jitless` is the best Score
reference at 5,037. Quench's median max RSS is 127,152,128 bytes; Bun no-JIT is
the lowest RSS reference at 60,010,496 bytes. Quench is at 41.4% of the Score
bar and uses 2.12x the RSS bar. Splay is not won.

`stock-candidate-node26-11.json` is a one-fixture diagnostic, not an all-eight
qualification report. The distinct-session trunk stock result is preserved in
`stock-trunk-node26-11.json`: Quench Score 1,437 and max RSS 119,754,752 bytes,
against Node 3,360 and Bun 60,395,520 bytes. Reference Scores also shifted
substantially between sessions, so this is not a causal speed comparison.

## Fixed-work diagnostics

`fixed-work-all4-node26-11.json` records 11/11 valid, output-equal rounds at
300 `run()` calls with setup-only subtraction. Linux `wait4` supplies no cycles
or instructions, leaving the contention detector's IPC check incomplete. It
therefore reports 0/11 clean rounds and no marginal speed summary. Do not infer
a speed win from this run. RSS remains recorded: Quench 126,976,000 bytes and
Bun 58,789,888 bytes for work; setup-only Quench is 67,469,312 bytes and Bun
47,554,560 bytes.

`paired-fixed-work-trunk-vs-collector-11.json` compares the exact trunk binary
against the collector candidate on the same fixture. It has 11/11 valid,
output-equal pairs but also 0/11 clean rounds, so it cannot support a timing
claim. Its valid RSS medians are 119,648,256 bytes for trunk and 127,500,288
bytes for the candidate (+7,852,032 bytes). This is a memory regression on the
diagnostic and a reason to re-evaluate the current collection policy before
retaining more collector complexity.

The distinct-session stock scores are sensitive to host contention and must not
be used to attribute changes. The next useful screen is a matched clean Linux
fixed-work comparison after resolving the missing hardware-counter/clean-round
signal, while keeping Splay as the sole active benchmark.
