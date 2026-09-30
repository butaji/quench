# Executing the rewrite queue

[`index.json`](index.json) is the sole authority for task identity, status,
dependencies, lane membership, pinned revisions, and `next_task`. Task files own
implementation intent and acceptance evidence; they do not redefine queue state.

Start with `next_task`. A task may start only after every `depends_on` item is
`done`. Independent tasks may be `in_progress` concurrently, including within
a lane when their dependencies allow it; `next_task` names the current
priority, not the only active task. Every `in_progress` item must have all its
dependencies `done`. Task 24 is the explicit convergence gate and task 27 is
the only production cutover; it deletes the legacy runtime at the correctness
gate. Pre-cutover work uses separately named development binaries; production
must never choose an engine through a flag or environment variable.

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
- Keep formatter-only churn out of semantic diffs where practical; isolate
  broad formatting changes so behavior and review evidence stay clear.
- Treat allocation failure, malformed residual data, interruption, re-entry,
  and unsupported platform mechanisms as explicit checked transitions.
- Store raw measurements and large reports under ignored `target/` paths with
  source, binary, toolchain, host, and command provenance.
- Cover fixed regressions with Rust unit tests, including specialized and
  unspecialized execution where caches or lowering can affect behavior. This
  follows the user's 2026-09-29 instruction to add regression coverage. The
  pinned Test262 inventory and unchanged harness remain the conformance gate;
  unit tests do not replace it. Later Node/Wasm gates retain their existing
  tracked/upstream suites. Task 58 owns the runtime unit suite's existing
  failures; no test is ignored, deleted to go green, or weakened.
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
(29–34, 57), Wasm materialization (38), and the Score/RSS gate (26) all depend on
it. Task 26 also follows cutover (27), so parity is reached on the single core. Before that point, a "Performance evidence" section asks for measurement
only: record the numbers and do not change code to move them. Existing
specializations (field/method caches, numeric arming, superinstruction rows)
stay as they are, frozen, and must pass task 42's
optimized-versus-generic gate at every change; they are not extended.
Representation work that removes duplicate authorities (35, 36, 44, 52) and Wasm
lowering (37, 39–41) is structural, not tuning, and may proceed.

## Kernel watermark

The next core is layered like a metacircular Lisp: a small kernel of primitive
operations, with every other semantic defined on top of it. The plan was to
declare the watermark before porting builtins. Instead, Test262 was closed first
with every builtin inside `impl Vm`. The order is now explicit, and each family
still moves only once:

1. Task 20 closes Test262 at 100%, and task 48 freezes that pass set as a fast
   ratchet.
2. Task 46 declares the watermark tables and splits the kernel and library
   crates. Tasks 12–17 move their families onto the kernel API. Every move is
   a ratchet diff with zero regressions.
3. Task 47 derives the generating extension from the tables and proves the
   Futamura equation against the reference kernel.
4. Task 59 reduces the kernel to a closed, named fundamental set. Every other
   kernel entry is accelerated, with a library reference definition, and the
   reference kernel alone passes all three conformance inventories.
5. Task 27 deletes the legacy runtime at the correctness gate (task 24). What
   remains is one core: a minimal metacircular kernel, a library defined
   through its API, and shared algorithm crates behind `AlgorithmBoundary`.

| Layer | Owns | May use |
| ----- | ---- | ------- |
| 0. Kernel | values, strings, heap, roots, shapes, storage, essential internal methods, Call/Construct, activations, realms, jobs, compiler, interpreter, specializers | nothing above it |
| 1. Language library | spec abstract operations, exotic-object semantics, all intrinsics and builtins | the kernel API only |
| 2. Algorithms | RegExp, Intl, Temporal, Date, numeric/string conversion (task 45) | the algorithm boundary trait only; no kernel or library types |

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

Test262 is not rewritten, and nothing is implemented twice. The next core now
passes stages 0–113, and for most areas it holds the authoritative
implementation. Legacy code is a source only for behavior the next core still
lacks, and it is deleted at task 27. As of 2026-09-29, Temporal, Intl and Date
are next-core authorities. Task 45 extracts them from the next core, not from
legacy (next `vm/temporal*.rs` is about 14.2k lines, legacy `temporal/` about
21k, and they share 26 of 374 function names). The next
core never calls into the legacy crate or shares its heap/value types, so
behavior moves by one of three routes, chosen by how coupled the legacy code
is to its engine:

| Legacy code | Examples | Route |
| ----------- | -------- | ----- |
| Algorithm behind a narrow value boundary (no interpreter, environment, or storage access; only conversions, property reads, errors, and intrinsic ids) | RegExp matcher, Intl/ICU, Temporal, Date, number/string conversion, BigInt, URI, Unicode casing/normalization | Move the passing implementation (next core where it exists, otherwise legacy) into a shared crate generic over the algorithm boundary trait (task 45); one authority, no copy |
| Builtin written against spec operations | Array, Object, String, Promise, TypedArray, Proxy, collection methods | Port file by file onto the kernel API (task 46): Get, Set, DefineOwnProperty, HasProperty, Delete, OwnPropertyKeys, Call, Construct; keep the algorithm, replace storage access |
| Evaluator, environments, heap, legacy reducer | — | Replaced by the v2 core (tasks 07–11); legacy is a behavioral reference only, and only proven edge-case logic is carried over |

The first route covers the largest block of remaining Test262 files. Legacy
Temporal, Intl, and Date (about 30k lines, roughly 10k Test262 files) reach
their engine through about a dozen operations, measured on 2026-09-24:

| Legacy operation | Call sites |
| ---------------- | ---------: |
| `ops::Builtin` intrinsic identities | 626 |
| `throw_range_error` / `throw_type_error` | 1,009 |
| `get_property_result` (may run guest getters) | 338 |
| `to_string` / `to_number` / `to_primitive` / `to_object` | about 430 |
| `is_object` / `is_symbol` | about 100 |
| `ObjectData` construction, `realm_intrinsic` | about 75 |

Legacy passes its VM implicitly (a thread-local machine and `Rc` values);
the shared crates take an explicit `cx: &mut impl AlgorithmBoundary` instead.
Moving a module is therefore mostly mechanical: legacy `Value` becomes the
boundary's opaque value type, and ambient engine calls become `cx` calls.
The substantive work is internal slots (guest state lives in a typed record
owned by the runtime, not in named properties) and rooting (a value held
across a `reenters` operation must be rooted by the next core's boundary
implementation).

The worklist is data, not directory order. For Test262 it is now the next
core's own failure report (task 20's frontier, then task 48's per-test
ratchet). For Wasm and Node it is still the legacy-pass/next-fail set that
task 19 freezes. Where a cluster names legacy code that already implements the
behavior, port that code before writing new code.

Migration order (from 2026-09-29):

1. Close Test262 on the next core (task 20). The legacy-pass/next-fail
   worklist is retired, because no stage is left in which legacy passes and
   the next core fails.
2. Freeze the next core's 100% pass set as the ratchet (task 48).
3. Declare the watermark and the `AlgorithmBoundary` rows (task 46). Extract
   next-core Temporal, Intl and Date into the shared crates, piloting on
   Temporal Duration (task 45).
4. Move the builtin families onto the kernel API (tasks 12–17), then reduce the
   kernel (task 59).
5. Delete legacy at cutover (task 27).

## Conformance ratchet

Task 48 freezes the next core's 100% Test262 pass set when task 20 closes.
Task 19 freezes per-test legacy outcomes for Wasm and `tests/node-compat`. From then on every change that touches the next engine,
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
