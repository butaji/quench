# Rust Host API Plan

This plan defines Quench's small, versioned Rust embedding API for hosts such
as `quench-node`. It is an additive layer over the existing OXC + interpreter
architecture; it does not change the Test262 runner or expose a C ABI.

Test262 runs are the only conformance source of truth. This work must not
compete with semantic foundations, runner throughput, or failure triage.
Each implementation milestone requires focused Rust unit tests first, then
the affected Test262 stage, with no changes under `tests/test262`.

## API boundary

The public boundary is a Rust crate API. It exposes isolate/realm lifecycle,
module registration, host callbacks, opaque rooted value handles, job driving,
and metrics hooks. Handles are isolate-local and must not expose `Value`,
object-layout, or `Rc`/`RefCell` details.

## Milestones

## Phase ordering

U0 and the smallest useful part of U1 belong to Phase 1. Property, conversion,
and callback expansion follow Phase 2 IR parity. Promise/heap-limit hooks and
multi-isolate deployment wait for the Phase 4 collector gate. Do not expose an
API merely because a later phase intends to support it.

### U0 — ownership and error contract

- Define isolate ownership, realm lifetime, thread affinity, and rooted-handle
  validity rules.
- Define whether returned values are borrowed or rooted handles; handles never
  cross isolates.
- Define explicit throw and exception-observation paths.
- Add crate API compile tests and lifecycle tests; no production API until those
  tests fail for the missing behavior.

### U1 — lifecycle and evaluation

Implement isolate and realm creation/destruction, script/module evaluation,
and global access through opaque rooted handles.

Map evaluation to the existing context evaluation boundary, preserving reset,
microtask, exception, and module semantics.

### U2 — property and primitive construction

Implement `get`, `get_idx`, `set`, `set_idx`, and constructors for undefined,
null, boolean, number, string, object, array, and ArrayBuffer. All property
operations route through the canonical runtime operations rather than adding
an API-specific storage path.

### U3 — type checks and conversion

Implement the mandatory type predicates plus `to_bool`, `to_number`,
`to_string`, and ArrayBuffer extraction. Conversion errors use the normal
pending-exception contract and never panic.

### U4 — functions and calls

Expose native function registration and:

Verify strict `this` behavior, argument ownership, thrown exceptions, and
native callback userdata with Rust tests before wiring the ABI.

### U5 — promises and limits

Expose promise creation and resolve/reject. Add runtime GC and heap/stack
limit hooks only after the collector/allocator has a real enforcement point;
until then these functions must not be exported as false success APIs.

## Compatibility mapping

| Universal concept | Quench implementation target |
| --- | --- |
| Runtime / Isolate | isolate-owned heap, collector, job queue, and intrinsic state |
| Context / Global | realm and its global environment |
| Value handle | rooted opaque wrapper around `Value` |
| Eval | `Context::eval` / module evaluation |
| Get / Set | canonical object property operations |
| Call | existing `call_value_with_this` path |
| Exception | pending `JsError` / thrown `Value` boundary |
| Promise | existing promise builtin and job queue |
| GC / limits | explicit allocator/runtime milestone, not a stub |

## Acceptance gates

For every milestone:

1. Add a failing unit test for the exact new contract.
2. Implement the smallest API surface that makes it pass.
3. Run the affected unit suite, formatting, and clippy with zero warnings.
4. Run the relevant Test262 stage and inspect its digest.
5. Commit and push the milestone independently.

The API work must not regress the 100% Test262 target, increase Rust core
duplication, or introduce a second spec-operation implementation.
