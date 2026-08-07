# Rust-to-JS Builtin Migration

Keep Rust limited to the interpreter core, canonical `__ops__`, storage and
native-memory primitives, engine integration, performance-sensitive or
crate-backed primitives, and documented lower-LOC direct bindings. Observable
ECMAScript algorithms belong in the self-hosted JavaScript layer whenever they
can be expressed over `__ops__`.

## Conformance-first gate

This migration plan is deferred until the complete configured Test262 corpus
is at 100% (zero failures and zero skips). Until then, only a minimal change
that directly fixes an observed conformance failure is allowed; do not migrate
a family or add complexity incidentally. Each migration begins from a complete
100% baseline and is accepted only after a second complete run proves that
100% is preserved.

## Rules

- Start each migration with a failing Rust refactor-pin or behavior test.
- JS implementations use canonical `__ops__` and never access Rust storage.
- Remove duplicate Rust registration only after JS owns the observable
  algorithm, validation, coercion, ordering, and descriptor behavior.
- Run the relevant Test262 stage before and after each family migration; the
  run is the only conformance result.
- A Rust direct binding is allowed only when it is a primitive, host
  integration point, or lower-LOC than a JS forwarding wrapper. Record such
  exceptions in `tasks/builtin-direct-bindings.txt`.
- `tools/check-builtin-ownership.sh` is the local ownership guard.

## Ownership queue

Migration is not automatically a throughput improvement. Select families
using failure fan-out, measured bootstrap cost, and total maintained LOC.
Shared semantic foundations and runner throughput take priority over
migrations that only relocate code.

The following families are migration targets or ownership boundaries. This
list is not a status ledger; Test262 output and source inspection determine
what remains for a particular change.

- `__ops__` bridge and bootstrap lifecycle
- Object and Reflect algorithms over canonical property and descriptor ops
- Array algorithms, array-like coercion, and result-property creation
- String coercion, matching, slicing, and replacement algorithms
- Number, Boolean, and Symbol conversion and prototype algorithms
- Map, Set, WeakMap, and WeakSet algorithms over hidden keyed storage
- Iterator, Generator, and AsyncIterator composition
- TypedArray, ArrayBuffer, DataView, and raw-buffer boundaries
- Promise algorithms over native reaction and scheduling primitives
- RegExp algorithms over crate-backed matching
- Date algorithms over chrono-backed calendar primitives
- Error, Function, Proxy, and constructor/prototype wiring
- DisposableStack and AsyncDisposableStack resource algorithms
- FinalizationRegistry and WeakRef host/GC integration
- Timer and other host integration points

For each family, keep only the Rust surface required by the runtime boundary
and move observable algorithmic behavior into `builtins/*.js`. A JS file that
only forwards to one Rust primitive is not a migration.

## Verification

For every family change:

1. Add and run the failing unit test.
2. Implement the smallest ownership change.
3. Run the unit suite, formatting, and clippy.
4. Run the affected Test262 stage and inspect its digest.

Do not copy digest results, stage claims, or completion markers into this
file.
