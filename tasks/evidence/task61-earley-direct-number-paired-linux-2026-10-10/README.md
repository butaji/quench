# Task 61: EarleyBoyer direct Number arithmetic (Linux, 2026-10-10)

The current v2-cloud baseline routes `+` through coercion and `*` through the
full numeric slow path even when both operands are already Numbers. This
candidate preserves the integer-pair addition shortcut, then returns direct
IEEE-754 addition for Number operands; multiplication gets the same Number-only
fast path. All other operand types retain the generic implementation.

Eleven alternating production pairs on the pinned EarleyBoyer fixture produced
equal semantic output in every run. Median Score rose from 207 to 213 (+3
paired median points; 95% bootstrap interval +1 to +9). Median maximum RSS fell
from 51,429,376 to 50,282,496 bytes (-782,336; interval -1,593,344 to
-286,720); RSS was lower in 10/11 pairs. The candidate Score was lower in two
pairs.

Node v24.19.0 and the exact candidate binary matched on 968 `+` and `*`
operations over 22 operand edge values, including signed zero, NaN, infinities,
safe-integer boundaries, strings, nullish values, BigInt and Symbol errors.
The candidate binary SHA-256 is
`e83200ec7666a02f8e4807fdb476eeb4dcef449ef469d15eefce1413f11369d5`;
the clean baseline SHA-256 is
`d6165b53df0198b58e5c3887fed1afad351bde17e237a6f711d495987cb79158`.

This is a focused Quench-only EarleyBoyer result, not an engine comparison or
Task 61 qualification. The contemporaneous one-round eight-fixture screen was
mixed, and the all-engine Task 61 Score/RSS gap remains open. The paired runner,
raw samples, candidate patch, and Node oracle are in this directory.

Online implementation context: V8's public Ignition `Add` handler routes
through `BinaryOpWithEmbeddedFeedback` in
[`interpreter-generator.cc`](https://github.com/v8/v8/blob/main/src/interpreter/interpreter-generator.cc#L1052).
That is a design reference only; the performance claim here comes from the
paired Quench measurements above.
