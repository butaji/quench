# Executing the rewrite queue

[`index.json`](index.json) is the sole authority for task identity, status,
dependencies, lane membership, pinned revisions, and `next_task`. Task files own
implementation intent and acceptance evidence; they do not redefine queue state.

Start with `next_task`. A task may start only after every `depends_on` item is
`done`. Independent tasks may be `in_progress` concurrently, including within
a lane when their dependencies allow it; `next_task` names the current
priority, not the only active task. Every `in_progress` item must have all its
dependencies `done`. Task 24 is the explicit convergence gate and task 27 is
the only production cutover. Pre-cutover work uses separately named
development binaries; production must never choose an engine through a flag
or environment variable.

Statuses are `pending`, `in_progress`, and `done`. `next_task` must identify a
non-done task whose dependencies are done. On completion, retain its Markdown
file, mark it `done`, and advance `next_task` to the highest-priority ready
task in the declared lane order. Set `next_task` to `null` only when every item
is done.

## Shared completion rules

- Follow the [repository rules](../AGENTS.md), including semantic separation,
  benchmark integrity, exact fallback, and Apple M4/macOS qualification.
- Preserve observable values, descriptors, identity, ordering, errors, exit
  status, output, and host effects. Changed Node behavior is checked against the
  local Node oracle and relevant pinned upstream source.
- Keep one authoritative value, heap, object, activation, opcode, and host-root
  representation. Derived metadata must be generated or validated from it.
- Treat allocation failure, malformed residual data, interruption, re-entry,
  and unsupported platform mechanisms as explicit checked transitions.
- Store raw measurements and large reports under ignored `target/` paths with
  source, binary, toolchain, host, and command provenance.
- During the current Test262-first phase, do not add Quench-owned tests or run
  unit-test suites. Use the pinned existing Test262 inventory and its unchanged
  harness for semantic verification. Later Node/Wasm gates likewise use their
  existing tracked/upstream suites; this rule does not waive those gates.
- A conformance command that discovers no tests is a failure, not evidence.
- Performance measurements never replace correctness evidence. Functional work
  may complete without a speed claim unless the task explicitly owns a
  performance gate.

## v2 design fidelity and binding time

The next core follows the pinned v2 design in detail. Its recorded design
decisions and measured negative results are copied under [`docs/v2/`](../docs/v2/)
(architecture, addressing domains, closure environments, inline-cache layout,
adaptive specialization, offline/runtime re-specialization, source
distillation). A task that departs from one of them must cite it and beat the
recorded measurement under the same paired Score/RSS gate; otherwise it follows
v2. Full-language additions extend v2's mechanisms; they do not replace them.

The compiler is a cogen-style generating extension (Futamura P1). Ershov's
mixed-computation question is asked of every new fact before choosing a
mechanism:

| Binding time | Examples | Mechanism |
| ------------ | -------- | --------- |
| Static (source, syntax, lexical scope, validated Wasm types) | literal keys, `arguments`/`eval` use, capture candidates, strictness, operand types | decided once in the generating extension and encoded in the residual; new proofs are a `StaticValue` variant plus transfer rule in `compile/binding_time.rs`, never a new ad hoc matcher |
| Specialization environment | atoms, constants, field/method/object sites, handler and root maps | compact tables indexed by instruction operands |
| Dynamic (values, shapes, effects, executed paths) | receiver shape, operand tags, whether a closure was created | runtime check with exact generic fallback; per-site state lives in flat VM-owned arrays indexed by site ID, never in the instruction stream |

After the kernel watermark (tasks 46–47), each row reads its facts from the
watermark tables. Static folding runs the statically evaluable kernel or
library definition, which is Ershov's one definition for both binding times.
The specialization environment addresses intrinsics and registry entries by
index. Dynamic fast paths are accelerated primitives whose exact fallback is
their library reference definition. Correctness is the Futamura equation,
checked against the reference kernel (task 47). Routing existing folds and fast paths
through the tables is structural. Adding new folds or accelerations is tuning
and waits for task 24.

Observed dynamic facts never become unguarded static facts, whether the
observation comes from a profile, a training run, or a counter. Per-object
facts live on the object's cell or shape; identity-keyed `Vm` hash tables are
not a semantic authority (see task 44).

## Correctness before performance

Task 24 (Test262 100%, Wasm 100%, and the frozen Node set green in one build)
comes before any performance tuning. Task 25's lab, the specializer lane
(29–34), Wasm materialization (38), and the Score/RSS gate (26) all depend on
it. Before that point, a "Performance evidence" section asks for measurement
only: record the numbers and do not change code to move them. Existing
specializations (field/method caches, numeric arming, superinstruction rows)
stay as they are, frozen, and must pass task 42's
optimized-versus-generic gate at every change; they are not extended.
Representation work that removes duplicate authorities (35, 36, 44, 52) and Wasm
lowering (37, 39–41) is structural, not tuning, and may proceed.

## Kernel watermark

The next core is layered like a metacircular Lisp: a small kernel of primitive
operations, with every other semantic defined on top of it. Task 46 declares
the watermark early, so builtins move only once. Task 47 derives the
generating extension from it and proves the Futamura equation. Task 48 makes
the full Test262 run fast enough to gate every kernel change.

| Layer | Owns | May use |
| ----- | ---- | ------- |
| 0. Kernel | values, strings, heap, roots, shapes, storage, essential internal methods, Call/Construct, activations, realms, jobs, compiler, interpreter, specializers | nothing above it |
| 1. Language library | spec abstract operations, exotic-object semantics, all intrinsics and builtins | the kernel API only |
| 2. Algorithms | RegExp, Intl, Temporal, Date, numeric/string conversion (task 45) | no kernel or library types |

The watermark between layers 0 and 1 is a crate boundary crossed only through
three declared tables:

- the **kernel primitive table**, which records each primitive's effect class
  and whether it is fundamental or accelerated;
- the **library operation registry**, one registry for builtins and opcode
  fallbacks alike;
- the **object-kind dispatch table** for exotic behavior.

An accelerated primitive must name a library reference definition written
with fundamental primitives only, so the kernel never adds semantics and a
reference kernel without accelerations must pass the same conformance run.
Every table row declares its effect class and static evaluability from the
start, so the staging rules hold before anything is derived from them.
Kernel experiments therefore change only the kernel. A change that forces
library edits is a change to the primitive table and is reviewed as one.

## Migrating legacy semantics

Test262 is not rewritten, and legacy semantics are not reimplemented from
scratch. Legacy `quench-runtime` already passes most of the suite; the next
core's job is to host that proven behavior on v2's representations. The next
core never calls into the legacy crate or shares its heap/value types, so
behavior moves by one of three routes, chosen by how coupled the legacy code
is to its engine:

| Legacy code | Examples | Route |
| ----------- | -------- | ----- |
| Pure algorithm (no `Value`, heap, realm, or interpreter types) | RegExp matcher, Intl/ICU, Temporal math, Date, number/string conversion, BigInt, URI, Unicode casing/normalization | Extract into a shared crate used by both runtimes (task 45); one authority, no copy |
| Builtin written against spec operations | Array, Object, String, Promise, TypedArray, Proxy, collection methods | Port file by file onto the kernel API (task 46): Get, Set, DefineOwnProperty, HasProperty, Delete, OwnPropertyKeys, Call, Construct; keep the algorithm, replace storage access |
| Evaluator, environments, heap, legacy reducer | — | Replaced by the v2 core (tasks 07–11); only proven edge-case logic is carried over |

The worklist is data, not directory order: task 19's frozen legacy outcomes
diffed against the latest next-core run give every test that legacy passes and
the next core fails. Each cluster in that diff names legacy code that already
implements the behavior; port it before writing new code. Tests legacy also
fails are the only ones that need fresh implementation.

## Conformance ratchet

Task 19 freezes per-test legacy outcomes for Test262, Wasm, and
`tests/node-compat`. From then on every change that touches the next engine,
the host facade, or a runner is checked against the latest recorded pass set
for each suite it can affect: newly failing tests are regressions and block the
change. Feature tasks close on their mapped conformance slices (task 20 maps
Test262 stages to tasks 11–18), so completion is measured, not asserted.

A foundation task (07–10) closes on its mechanism contract. Producers in later
tasks adopt it as part of their own definition of done, which keeps the
dependency graph acyclic.

## Final gates

- Every official test discovered from the pinned Test262 checkout passes; there
  is no Quench-owned feature skip list.
- Every directive discovered from the pinned WebAssembly testsuite passes.
- Every tracked `tests/node-compat` fixture that the legacy engine implements
  (task 19's frozen passing set) matches the local Node oracle on the next
  engine.
- The standalone interpreter meets the pinned `../v2` reference on both median
  Score and median maximum RSS for Richards, DeltaBlue, Crypto, and Splay.
- The final tree contains no guest JIT, copy-and-patch stencil, executable-memory
  runtime, legacy backend, or sibling-checkout build dependency.

The generic Lisp-mindset skill suggests size caps, but this repository's rules
explicitly reject mandatory line-count or complexity ceilings. Cohesion and
reviewability remain required; no numeric cap is imposed.
