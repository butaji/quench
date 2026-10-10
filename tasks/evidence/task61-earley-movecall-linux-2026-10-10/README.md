# Rejected EarleyBoyer `Move → Call` argument forwarding

The scoped EarleyBoyer profile reported 17,248,857 adjacent `Move → Call`
dispatches. I tried forwarding a one-argument call window directly to the
argument's source register, and forwarding the moved source into the call's
callee or receiver fields when those fields used the copy. Multi-argument
windows kept the existing copy because the call ABI requires contiguous
registers. The rewrite also kept a fallback when the moved destination was the
call result or an argument in a multi-register window.

An instrumented EarleyBoyer compile selected 18 static rewrite sites. This is
not a dynamic execution count; the benchmark profile's adjacent-pair count
does not reveal which of those sites executed or how often. The candidate
matched Node v24.19.0 on single-argument calls, multi-argument fallback,
receiver mutation, direct eval, and caught object exceptions. EarleyBoyer
output matched in all 11 alternating production pairs.

The source was based on `ef02863ec059d41ffe6a92f2ddf5f2de44773f38`. The
baseline binary SHA-256 is
`e46537823eb25a37cf5646af793e6873ceaeff1069fb878a96813cae3a86ab32`; the
candidate binary SHA-256 is
`bde238a2a6d6b8c7d13e555313facabd05f3708e73a478b0a8f1dd15936fcf60`. The
materialized V8-v7 EarleyBoyer input SHA-256 is
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

Eleven alternating Linux x86_64 production pairs gave a median Score delta
of 0 points (paired bootstrap 95% interval −4 to +3). Median maximum RSS
increased by 77,824 bytes (interval −69,632 to +159,744), and candidate RSS
was lower in only four pairs. Both intervals include a tie, and the candidate
did not establish a Score or memory win, so I removed it.

V8's [Ignition interpreter article](https://v8.dev/blog/ignition-interpreter)
describes reducing register transfers as a bytecode optimization motivation.
That is a hypothesis for measurement, not evidence that this rewrite helps
Quench. The 11-pair result rejects this implementation on EarleyBoyer.

`paired-11.json` and `pairs-11.jsonl` contain the paired results and bootstrap
summary. `raw/` contains the ordered per-run stdout/stderr. `candidate.patch`
preserves the rejected source change without the temporary counter used to
count static rewrite sites. `node-oracle-report.json`, the five oracle inputs,
and `rewrite-site-count.txt` record the focused semantic checks and static
rewrite count. The runner records the pinned fixture, environment, and
binary hashes.
