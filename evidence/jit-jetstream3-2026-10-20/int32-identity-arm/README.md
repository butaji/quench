# Guarded Int32 identity arm

This experiment recognizes a bytecode path that dispatches on
`typeof argument === "number"`, checks `(argument | 0) == argument`, then
returns the original value. The matcher simulates the pure Number dispatch
prefix, rejects effects along that path, and emits a native identity function.
The VM takes the shortcut only after an Int32 guard. Other inputs, including
fractions, strings, objects and out-of-range numbers, continue through the
interpreter. The matcher uses bytecode and constants; it does not check source
or function names.

The focused runtime regression covers integer results and interpreter
fallbacks for fractions, strings, and objects. `cargo test --locked -p
quench-runtime js_jit_compiles_guarded_int32_identity_arm_and_falls_back_for_other_types
--lib` passed.

The stock JetStream 3 `hash-map` validation passed. The diagnostic report
records `computeHashCode` compiled once and entered natively 64,800,000 times,
with no interpreter fallback for that function. `_findKeyEntry` remained
interpreted for 54,000,000 entries and dominated inclusive interpreter time
(26.9 s in this diagnostic, which is not exclusive CPU time). `runIteration`
also remained interpreted. The separate clean one-round screen measured:

| Runtime | Score | Peak RSS |
| --- | ---: | ---: |
| Quench JIT | 3.51 | 73.9 MiB |
| Bun JIT | 471.06 | 111.5 MiB |
| Node JIT | 456.95 | 172.6 MiB |

This screen is not a qualification and does not show a score win. A prior
one-round screen on the parent revision measured Quench at 4.35, Bun at
467.12, and Node at 414.81, with different RSS values; the independent
screens cannot establish a regression or an improvement. The large remaining
score gap points to the linked-entry lookup loop and its repeated heap and
call work, not the integer hash calculation.

Raw clean and diagnostic JSON reports are stored beside this file. The
diagnostic run used `--jit-diagnostics`; its score is excluded from comparison.
