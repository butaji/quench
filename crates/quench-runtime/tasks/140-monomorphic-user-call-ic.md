# 140 — Monomorphic user-call inline cache

Status: complete

Fresh accepted Task 137 profiles show recursive
`dyn_block_step_impl -> Vm::call_arguments -> DynJitCode::run_with_locals` as the
dominant Earley-Boyer stack. Task 139 proved that decorating numeric argument copies
adds overhead. Instead cache the stable user-function dispatch result per general
`Call` bytecode: callee identity, compiled `DynJitCode`, and lexical environment.

The cache is write-once and monomorphic. A matching callee bypasses repeated function
kind tests, numeric/dynamic `RefCell` probes, and per-call code-image `Rc` cloning. A
different callee takes the canonical dispatcher forever; it never replaces storage that
an active recursive call could reference. The cache retains its callee graph, making
its raw identity pointer valid for the linked caller image lifetime. Native functions,
constructors, exceptions, receiver semantics, and argument ownership remain unchanged.

This is a general bytecode call-site morphism selected for every `Call`, independent of
source name, property spelling, benchmark identity, or hotness. The call remains an
effect kernel; the IC specializes its dispatch obligation.

Acceptance: mono/polymorphic and recursive semantic tests, full release suite and V8v7
smoke, gated hit/miss evidence, then alternating full-suite A/B against Task 137. Reject
and revert unless the confirmed aggregate improves without violating suite floors.

## Result

Accepted. Every linked dynamic function now owns one write-once `CallIcSite` per
bytecode PC. The first user-function call stores the callee identity, an owning callee
word, its shared compiled image, and lexical environment. Matching calls invoke the
cached image directly; polymorphic mismatches retain the canonical dispatcher and
cannot replace storage referenced by an active recursive call. Native, numeric-image,
receiver, argument, exception, and stencil-only failure behavior is unchanged.

All 76 release tests and the complete eight-suite smoke pass. The first four-run full
A/B in `reports/task140-call-ic-full-ab-4/comparison.txt` improved aggregate 1.39%.
The longer six-run confirmation in
`reports/task140-call-ic-full-ab-6/comparison.txt` remained positive at 1152.39 to
1162.35, or +0.86%; every suite remained above the standing -5% floor. An uninstrumented
three-second Earley-Boyer native sample in
`reports/task140-earley-call-ic.sample.txt` reduced top-of-stack generic call dispatcher
samples from the Task 137 profile's 67 to 34, and `dyn_block_step_impl` from 431 to 371.
This ties the improvement to the intended dispatch path without adding hot-path
counters.

Accepted binary: `/tmp/deegen-task140-call-ic`, SHA-256
`4bd299b06743af0cab92f60ba30032d6cf8340f7810bd853abecd1b70b7f946c`.
