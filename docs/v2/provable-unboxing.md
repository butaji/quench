# Provable numeric unboxing

Unboxing without a runtime guard requires a source-level proof over every
control-flow predecessor. Dynamic profiles may rank a region after it is
proved, but cannot participate in the proof.

## Required type lattice

A sound extension of the binding-time pass would use a lattice at least as
precise as:

- `Bottom`: no reaching definition;
- `Int32`: every reaching definition is a representable signed 32-bit integer;
- `Number`: every reaching definition is a JavaScript Number, but not
  necessarily an `Int32`;
- a fixed non-number primitive kind;
- `Dynamic`: reaching definitions disagree or the value comes from an
  unproved edge.

Parameters, `this`, global/name loads, field loads, indexed loads, calls, and
coercions from dynamic inputs begin as `Dynamic`. Numeric literals are
`Int32` or `Number`. Integer addition and multiplication remain `Int32` only
with a range proof; otherwise they widen to `Number`. JavaScript `+` is
numeric only when both inputs are already proved numeric, because its dynamic
case includes string concatenation. A loop header joins the back edge with the
entry edge until a fixed point. Any conflicting definition makes the local
dynamic for the region.

An unboxed region must also describe its boundaries: values are boxed before
calls, property/index writes, returns, throws, captured-environment stores, or
entry into ordinary bytecode. Exception successors participate in the same
proof. Without those rules, raw values can leak into heap/GC-visible storage.

## Residual audit

The current data-derived Numeric dispatch class provides the natural place to
look for a guard-free region. Its complete retained-workload inventory is:

| Workload | Numeric functions | Qualifying hot locals |
| --- | ---: | ---: |
| Richards | one harness function, 0 locals | 0 |
| DeltaBlue | one harness function, 0 locals | 0 |
| Splay | one harness function, 0 locals | 0 |
| Crypto | `am2`, `am3`, `am4`, `bnpInvDigit`, one harness function | 0 in the hot `am3` body |

Crypto's important case is `am3`, called about 3.10 million times. All six
parameters arrive through a method call, so their source type is dynamic. The
body derives locals from those parameters, `this`/receiver fields, or indexed
array reads. Its incremented/decremented loop variables are parameter-derived,
not constant-initialized. Although aggregate profiling observes integers, no
source fact proves them numeric for every valid call. `bnpInvDigit` likewise
starts from receiver fields/indexed data and is far colder.

Constant-initialized counters elsewhere can be proved numeric, but they are
mixed into general functions whose operations cross boxed calls and heap
edges. Adding a parallel raw-local store, new addressing mode, and boxing
boundaries for those cold fragments would tax frames/dispatch without
removing checks from the dominant numeric body.

## Decision

No retained workload contains a hot region satisfying the no-guard proof
obligation, so there is no sound unboxed candidate to benchmark. Treating the
observed `am3` integers as proof would reproduce task 14's speculative type
feedback under a different name. rqj keeps the existing boxed `Value` numeric
dispatcher and its checked integer fast path. This task can be reopened when
whole-program call-target/type analysis proves a hot function's inputs, or a
future benchmark contains a hot constant-origin numeric region.
