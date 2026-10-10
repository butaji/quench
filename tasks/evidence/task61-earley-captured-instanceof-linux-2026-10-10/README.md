# Rejected EarleyBoyer captured `instanceof` fusion

The scoped profile showed 16.65 million `LoadCapture → Binary` adjacencies,
mostly `value instanceof capturedConstructor`. I added a fused opcode for that
case. It reads the capture, writes it to the original temporary register, then
runs the existing `instanceof` implementation and writes the Boolean result.
Keeping the temporary write preserves ordering and uses on exceptional paths.
The opcode metadata also treats the operation as a call that may read, write,
and throw; the capture contributes to closure-layout validation and profiling.

The Node v24.19.0 oracle covered ordinary and bound functions, mutated capture
bindings, custom `@@hasInstance` callbacks and exceptions, proxy traps, and a
capture read during the temporal dead zone. Baseline and candidate outputs
matched Node exactly, including error messages and observed trap order.

Three alternating production pairs on the pinned EarleyBoyer input matched
benchmark output but consistently regressed:

- Median Score: 416 baseline, 386 candidate; paired median delta −30.
- Median maximum RSS: 41,779,200 bytes baseline, 47,628,288 candidate; paired median delta +5,804,032 bytes.
- Candidate Score was lower and RSS higher in all three pairs.

The candidate binary grew by 95,840 bytes; the observed RSS increase is not
explained by a multi-megabyte executable-size change. This was a three-pair
rejection screen, not a qualification interval. I removed the opcode and
rewrite. Exact hashes, ordered pairs, raw output, oracle, runner, and patch are
in this folder.
