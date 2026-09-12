# 428 — Type-mask overapproximation automaton for guard generation

Status: planned

Deegen generates type checks from declared type sets (`tDouble`, `tHeapObject`, arbitrary
unions) via a build-time automaton generator
(`deegen/typemask_overapprox_automata_generator.cpp`): rather than hand-writing, for every
possible declared type mask, the minimal sequence of tag comparisons that recognizes it, the
tool derives an automaton over the tagged-value bit layout and emits the smallest correct
check sequence per mask, including safe *overapproximation* when an exact check would be more
expensive than a slightly broader one that a later, cheaper check can narrow. This turns "what
is the fastest correct guard for this exact proven-type set" into a generated, provably-
correct lookup instead of a per-opcode-author judgment call.

Quench's guard-elision and speculative-guard tasks (25, 60, 223, 231-234, 261 "known-bits tag
analysis") prove guards can be removed or bounded, but grep found no task about *generating
the check itself* from a declared type-mask as a build-time-derived minimal automaton; today
each guarded stencil presumably hand-encodes its tag comparison. This is complementary, not
redundant: elision tasks decide *whether* a check is needed; this task is about the artifact
that gets emitted for the checks that remain.

Acceptance: a generator that, given an arbitrary subset of Quench's tagged-value type lattice
(`TaggedValue`/`DecodedValue` variants in `tagged_value.rs`), emits the minimal correct
comparison sequence, verified exhaustively against a reference truth table for every subset up
to the current tag count; at least one existing hand-written guard replaced by a generated one
with identical or better instruction count; full correctness suite.

Primary source: luajit-remake `deegen/typemask_overapprox_automata_generator.cpp` and
`test_typemask_automata_generator.cpp`
<https://github.com/luajit-remake/luajit-remake/tree/master/deegen>.
