# 96 — Guarded numeric binary fast kernel

Status: complete

Generate a compact numeric specialization alongside every binary operation from the
same `define_ops!` declaration. The inlined semantic kernel checks the two word-sized
operands once; number pairs execute the generated `f64` operation locally, while every
nonnumeric pair calls the unchanged generic JS coercion/string/equality semantics.

This is a typed coproduct elimination: the `Number × Number` injection gets a closed
machine-word morphism and the remaining value sum takes the generic kernel. It is not a
second bytecode definition—the enum, generic implementation, and numeric specialization
are emitted by one Rust macro invocation.

Acceptance: numeric and coercion/string semantic tests; full smoke; binary-size record;
balanced full-suite A/B under the standing gates.

Result: `define_ops!` now emits the `Op` enum, generic JS operation, and guarded numeric
specialization from one declaration. A unit test checks every generated operation over
zero, negative, infinity, and NaN against generic number semantics, plus string
concatenation. The release executable is 2,990,736 bytes. Forty-one release tests and
all eight suites in `reports/numeric-fast-kernel/smoke.jsonl` pass.

The preserved exact predecessor for task 94 had already been overwritten by the
rejected task-95 build. The balanced four-repetition comparison against task 93 is
therefore recorded transparently in `reports/numeric-fast-kernel-ab-vs93/comparison.txt`:
the combined task-94+96 build is +19.50% versus task 94 alone's earlier +16.90%.
Comparing suite medians across those two balanced records puts the incremental result
at about +1.4% aggregate, with RayTrace -2.9% the largest suite movement and all others
inside the standing floor. The specialization remains enabled.
