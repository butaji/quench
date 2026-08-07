# Principles — keeping the codebase small and clear

These govern every change to the Rust core and JS builtins. They
exist to serve the minimum-LOC goal; when a choice
isn't covered here, prefer the option with less *total* code.

## Conformance before complexity

Until a complete configured Test262 run has zero failures and zero skips, do
only the smallest targeted conformance fixes. Do not use an observed failure as
an opening for a refactor, migration, new abstraction, architecture change, or
performance project. After the corpus reaches 100%, those broader changes must
preserve it: establish a complete passing baseline before the change and run
the complete corpus again after it.

## Effects live in return types, not in ambient state

Any information that flows between caller and callee — completions,
errors, thrown values — travels in the return type (`Result`, enums)
and propagates with `?`. Thread-locals and side channels are for
state that genuinely cannot be passed (e.g. `CURRENT_CONTEXT`), never
as a substitute for a return value. Manual "take → match → re-set"
plumbing is a defect: it means a type is missing. Introduce the type;
let the compiler check propagation.

## Ambient state shrinks toward zero

Every `thread_local!` slot is a liability: set/take/peek accessor
triplets, stale-state bugs across calls, invisible coupling. Passing
state explicitly (a context parameter, a realm field) is always
preferred. Before adding a slot, prove the state can't ride on the
call stack.

## Use the spec's own types

When an internal enum or struct re-encodes a spec concept (completion
kinds, property descriptors, iterator results), it is an adapter —
delete it and carry the spec concept's type directly. One type per
spec concept, defined once, named as the spec names it.

## Functions before macros

A macro is admitted only where a function cannot erase the
repetition: variadic formatting, or generating named items. Macros
that hide control flow or logic are forbidden — test262 failures must
land on greppable, steppable code. Registration-style repetition is
solved with data (const tables + one registration function), not with
macro DSLs. A macro used fewer than ~10 times or saving less than ~2
lines per site is not worth its definition.

## Tables over code

Method lists, property sets, and dispatch that repeat one shape
belong in a const table consumed by a single interpreter function.
The table reads like the spec's own list; adding an entry adds a row,
not a copy of the plumbing.

## Repeated idioms become named one-liners

Any 2–3 line idiom appearing ~10+ times (error construction, argument
defaults, receiver fetch) becomes one named function, once. Call
sites should read as the spec step they implement
(`throw_type_error(...)`), not as its mechanism.

## One canonical path per operation

Restated from `AGENTS.md` because it is the same principle: every
spec abstract op, every error constructor, every protocol (iterator,
property access) has exactly one implementation that all callers
route through. A second path — even a shorter local one — is
duplication and is deleted in the same PR that notices it.
