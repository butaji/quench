# Executing the rewrite queue

## Priorities and phases

The queue delivers one result: the best possible efficiency (lowest maximum
RSS, highest Score) from an interpreter-only engine, reached by
Futamura/Ershov staging, while keeping 100% of the pinned Test262 inventory,
100% of the pinned WebAssembly testsuite directives on the same VM, and every
covered Node compatibility fixture (task 19's frozen set). Two phases run in
this order:

1. **Correctness and cleanup (phase A, gate task 27).** All three suites green
   in one build on the shared JS/Wasm VM (task 24), then cutover (task 27):
   the legacy runtime and every migration-only or stale artifact is deleted,
   leaving one VM and a clean tree.
2. **Efficiency (phase B, gate task 28).** Lowest maximum RSS and highest
   Score on all eight V8-v7 fixtures against QuickJS, Bun/JSC with its JIT
   disabled, and Node/V8 `--jitless` (task 61), with no JIT of any kind. Two
   tracks start at cutover:
   - **v2 parity:** `../v2` is the reference for reaching high scores fast,
     so its measured mechanisms are ported first (tasks 25, 49, 50, 26).
   - **Staging spine:** the watermark tables (46) and the generating extension
     derived from them, with the reference kernel as oracle (47). The
     static-fact specializers (57, 70–76) and builtin accelerations (85) build
     on them.

   Task 61 then goes beyond v2 using task 77's coverage map, and task 28
   closes the queue.

`phases` in [`index.json`](index.json) lists each phase's tasks and gate, and
`depends_on` encodes the gate: no phase-B task starts before task 27. Phase A
may measure but not optimize. Tasks already `in_progress` when the phases were
declared (45, 48) finish in place. Task 62 adds reference engines to the
runner, task 68 audits static-fact opportunities, and task 77 attributes time
and RSS to the efficiency model's terms; all three are measurement only, so
they may run in any phase.

JavaScript and Wasm share one VM. There is one heap, one `Value`
representation, one root set, one `opcodes!` vocabulary, one dispatch loop, one
activation stack, and one `Engine` entry (tasks 37, 40, 23). `quench-wasm`
owns decoding, validation, and wast scripting only; no other crate executes
Wasm. Task 24 checks this structurally.

## Queue mechanics

[`index.json`](index.json) is the sole authority for task identity, status,
dependencies, lane membership, phases, pinned revisions, and `next_task`. Task files own
implementation intent and acceptance evidence; they do not redefine queue state.

Start with `next_task`. A task may start only after every `depends_on` item is
`done`. Independent tasks may be `in_progress` concurrently, including within
a lane when their dependencies allow it; `next_task` names the current
priority, not the only active task. Every `in_progress` item must have all its
dependencies `done`. Task 24 is the explicit convergence gate and task 27 is
the only production cutover; it deletes the legacy runtime at the correctness
gate and closes phase A. Pre-cutover work uses separately named development
binaries; production must never choose an engine through a flag or environment
variable.

Statuses are `pending`, `in_progress`, and `done`. `next_task` must identify a
non-done task whose dependencies are done. On completion, retain its Markdown
file, mark it `done`, and advance `next_task` to the highest-priority ready
task in phase order, then in the declared lane order. Set `next_task` to `null` only when every item
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
| Dynamic, dependency-guarded (shared state that is rarely written) | prototype chain of a cached receiver, `Array.prototype[@@iterator]`, `@@species`, intrinsic identity behind a `CallKnown` | one named validity cell (a *protector*, declared as a task 46 fact row) owned by the cell or shape whose mutation would break the fact; the guard is one cell read, and the mutation is the explicit transition that trips it. It is still a guard, not a static fact |

After the kernel watermark (tasks 46, 47), each row reads its facts from the
watermark tables. Static folding runs the statically evaluable operation row
(empty effect set), which is Ershov's one definition for both binding times.
The specialization environment addresses intrinsics, operation rows, brands,
and fact cells by index. Dynamic fast paths are `fast` entries on operation
rows, guarded by fact rows, whose exact fallback is their row's library
reference definition. Correctness is the residual run matching the reference
kernel (no fast paths, no specialization) on all three suites (task 47).
Routing existing folds and fast paths through the tables is task 47; adding
new folds or accelerations belongs to the specializer tasks that depend on it.

Observed dynamic facts never become unguarded static facts, whether the
observation comes from a profile, a training run, or a counter. Per-object
facts live on the object's cell or shape; identity-keyed `Vm` hash tables are
not a semantic authority (see task 44).

Every guarded fact has one provenance: a static proof, a per-site guard, or a
protector. The fact itself is one row of task 46's fact table, whatever its
provenance. A mechanism that needs a new kind of provenance extends this
table. It does not add a parallel cache or profile system or its own guard
vocabulary.

Per-site adaptive state is a named state machine over a finite lattice whose
transitions only move up: inline-cache degree (uninitialized ⊑ monomorphic ⊑
polymorphic ⊑ megamorphic), numeric arming (generic ⊑ armed ⊑ disarmed),
shape field representations, and array element kinds (task 50). A site moves
down only at a named reset (GC boundary, realm teardown, program
initialization). Its number of transitions is therefore bounded by the
lattice height, so re-arm/deoptimize oscillation is impossible by
construction. It is not bounded by a tuned retry count.

Every guard names its resume point: the generic instruction, and the register
state, from which the unspecialized path continues when the guard fails. For
single-instruction sites this is the site itself. For fused windows (task 33)
and binding-time-selected opcode sequences it is a derived map from the fused
position to the unfused instruction. Exact fallback means continuing generically from the resume point is
observably equal to continuing specialized; tasks 33 and 76 force each guard
to fail to check it, and task 42 checks it suite-wide.

### Static facts before execution

A loaded script or module carries more static structure than the current
analysis uses: `compile/binding_time.rs` is flow-insensitive, covers only the
root function, and knows only exact constants and function identities. The
catalogue below lists every fact domain the generating extension can compute
before execution. Each domain belongs to one binding-time class, one
authority, and one consumer task. Task 68 counts the opportunities for every
row. Consumers start in the order of task 68's counts. Every domain is added
to the one binding-time analysis in `compile/binding_time.rs`, flow-sensitive
and over every function, with transfer rules read from task 46's tables. No
consumer adds a separate pass with its own copy of scope or control-flow
knowledge.

| Domain | Evidence before execution | Class | Invalidated by | Consumer | Task |
| ------ | ------------------------- | ----- | -------------- | -------- | ---- |
| Closed scope | syntax: no direct `eval`, no `with`, no sloppy `arguments` aliasing or `Function.prototype.caller` exposure in or around the scope | Static | — | gates every row below; an open scope keeps the generic forms | 57 |
| Scope resolution and slot addresses | OXC semantic scopes and declaration instantiation | Static in closed scopes | direct `eval`, `with` | local, environment and capture opcodes | 10, 32, 49 |
| Capture and write sets per binding | syntax | Static | direct `eval` | environment layout; captured bindings with no writes become constants | 32, 81 |
| Single assignment and constants | dataflow over closed scopes, in every function, not only the root | Static | a second reaching write | `LoadConst`, folds | 57 |
| Value kinds, refinements, integer ranges, induction variables | dataflow with task 46's result-kind columns | Static | `reenters`/`throws` edges for heap facts | numeric opcode choice, `typeof` folds, TDZ and nullish elision, indexed-versus-named key selection | 57 |
| Module and global references | the module graph; non-writable, non-configurable globals (`undefined`, `NaN`, `Infinity`); script-level lexical declarations | Static for module bindings, frozen globals and `const` lexical globals | TDZ only | direct slots and constants | 70 |
| Global object properties and intrinsic identity (`Math.floor`, `Array.prototype.push`) | none: guest code can replace them | Dependency-guarded | a write to the property | global property cells, `CallKnown` behind a protector | 70 |
| Call graph | known callees from bindings that are never reassigned; arity; use of `arguments`, `this`, `new.target`, `super`; strictness; recursion | Static for known callees | unknown callee | `CallKnown`, direct argument binding, no `this` coercion | 71, 31 |
| Effect summaries | task 46's effect columns joined over the call graph | Static | unknown callee counts as `reenters` | heap facts survive calls; handler and root-map elision | 71 |
| Allocation-site shapes | object and array literals with static keys and no `__proto__`, spread or computed key; class bodies; constructor `this.x =` sequences | Static for literal and class layouts; a layout hint for constructors | none for literals; a hint never changes semantics | allocate at the final shape; reserve inline slots; pre-intern transitions | 72 |
| Allocation escape | dataflow plus effect summaries and allocation-site shapes | Static | a `reenters` edge, an unknown call, or an iterator protocol without a protector | scalar replacement of literals, destructuring temporaries, iterator results and `arguments` objects | 73 |
| Static control flow | constant conditions, unreachable code, natural loops, loop-invariant pure operations, `try` bodies that cannot throw | Static | — | smaller residuals, fewer dispatches, no handler setup | 74 |
| Liveness across suspension points | dataflow over generator and async bodies | Static | — | suspended-frame layout, register coalescing | 50 |
| Literal preparation | RegExp literal pattern and flags; template and constant strings | Static | — | a matcher compiled once per literal site with static pattern facts (literal prefix, anchoring, capture count, backtracking-free classes) | 75 |
| Known-callee bodies | call graph plus effect summaries | Static | stack traces, sloppy `caller`/`arguments.callee`, the recursion budget | inlining in the residual with declared resume maps | 76 |
| Reachability | the call graph in closed scopes | Static | direct `eval`, dynamic property access to functions | lazy lowering order | 67 |
| Wasm immutables | validation and link-time table contents | Static | — | constant and known-call materialization | 38 |

Receiver shapes, parameter kinds, property existence, and observed types are
not in the catalogue. They are dynamic (see task 57's evidence table), so they
belong to guarded caches and never to this analysis.

### Prior art

The rules above re-derive, for an interpreter-only core, results that earlier
dynamic-language VMs reached with JITs. This index names the lineage so that a
reviewer can check a proposal against the known failure modes. It adds no rule
of its own.

| Rule | Prior art | Known failure mode it avoids | Tasks |
| ---- | --------- | ---------------------------- | ----- |
| One residual bytecode is the meaning; every tier specializes it | V8 after full-codegen/Crankshaft (Ignition feeds every tier); HotSpot bytecode; Truffle (P1 in production) | two front ends drifting into two semantics | 10, 37, 40, 42, 47 |
| Immutable shapes with shared transition chains | Self maps; V8 hidden classes and field-representation tracking | quadratic transition cost; silent drops to dictionary mode | 08, 44 |
| Inline-cache states are the type profile | Deutsch–Schiffman inline caches (Smalltalk-80); Hölzle's polymorphic inline caches (Self); V8 feedback vectors | a separate profiler; unbounded polymorphic lists; eager feedback allocation | 08, 29, 30 |
| Dependency guards (protectors, validity cells) | V8 prototype validity cells and protectors; HotSpot class-hierarchy dependencies | re-checking every prototype level on every hit | 29, 34, 57 |
| Monotone adaptive state | V8 elements-kind lattice; HotSpot/V8 reoptimization limits | deoptimization loops | 08, 30, 50 |
| Resume points and exact fallback | Self-92 deoptimization; HotSpot uncommon traps and scope descriptors; V8 frame states | fast paths that cannot resume exactly | 33, 42, 76 |
| Customization only where measured | Self-91 customization and splitting, then Self-93 adaptive recompilation | code-size blowup from eager specialization | 32, 33, 57 |
| Optimize from guarded facts, not declared types | Strongtalk (optional types ignored by the optimizer) | static annotations standing in for guards | 57 |
| Primitives with a reference fallback | Smalltalk primitive failure falling into the method body; V8 Torque builtins | fast path and slow path with two definitions | 46, 47 |
| Kernel with library-defined semantics | Squeak VMMaker (Slang); Self's Klein | a kernel that grows semantics | 46, 47 |
| Capabilities at the edge | Newspeak (Strongtalk's successor): no global state | ambient host effects inside the core | 43, 53, 54, 55 |
| Startup heap as a derived artifact | Smalltalk images; V8 context snapshots | a hand-maintained snapshot that drifts from initialization code | 67 |
| Compressed references and a young generation | Ungar's generation scavenging; HotSpot compressed oops; V8 pointer compression and Orinoco | per-object `malloc` and a full-heap trace per collection | 07, 50, 82 |
| No constructs excluded from fast paths | Crankshaft's "optimization killers" | performance cliffs | 10, 31 |

## Efficiency model

Phase B optimizes two measured quantities on each V8-v7 fixture: Score, which
is work per unit of wall time, and maximum RSS. Every efficiency task derives
from the cost model below. It names the term it reduces, and it shows the
change on that term and on the paired gate.

```text
time    = Σ_op N(op) · (dispatch + decode + work(op))      interpreter
        + runtime library + allocation + collection         runtime
        + memory stalls                                     every term above
max RSS = touched binary text and data + stacks
        + max over t of (live heap(t) · (1 + header overhead) · (1 + GC headroom)
                         + fragmentation)
        + residual and metadata + allocator overhead
```

| Principle | Terms it reduces | Owner tasks |
| --------- | ---------------- | ----------- |
| 1. Remove work at the earliest binding time: static, then load time, then guarded, then generic | `N(op)`, runtime library | 57, 67, 70–76 |
| 2. Make the common dispatch the cheapest path: state in machine registers, operands the verifier has proved, one indirect branch per operation, cold paths out of line | dispatch, decode | 33, 78, 84 |
| 3. Let representation carry the fast path: NaN-boxed values, shapes with inline slots, element kinds, and a Latin-1/rope/slice string lattice | `work(op)`, live heap | 06, 08, 50, 79 |
| 4. Allocate less, allocate by bumping a pointer, and reclaim young objects cheaply | allocation, collection, live heap | 72, 73, 81, 82 |
| 5. Bytes are time: smaller hot structures raise cache hit rates on the host's measured cache sizes and 16 KiB pages | memory stalls, RSS | 50, 77 |
| 6. Touch fewer pages: create lazily, derive at the boundary, return pages to the OS | text/data, heap, startup | 67, 82, 83 |
| 7. Choose each policy on the Score × RSS Pareto front under the paired gate, never on one axis | all | 26, 61, 77 |
| 8. Accelerate a builtin only through a kernel primitive with a reference definition | runtime library | 47, 75, 80, 85 |
| 9. Key every mechanism on semantics, never on fixture identity ([AGENTS.md](../AGENTS.md)) | — | all |

Task 77 owns the attribution tooling and the
[coverage map](77.md#coverage-map), which lists every known mechanism with its
term, the fixtures it affects, prior art, v2's record, and its owner task.
Task 61 closes only when every map row has a retained mechanism with paired
evidence or a recorded negative result.

## Phase order

Task 24 (Test262 100%, Wasm 100%, and the frozen Node set green in one build
on the shared JS/Wasm VM) and cutover (27) close phase A. Cutover also removes
everything that exists only for the legacy engine or the migration (task 27's
clutter list), so phase B starts from one VM and a clean tree.

Phase B runs on that single core under task 48's ratchet over all three
suites. The v2-parity track (25, 49, 50, 67, then 26) and the representation
work (44, 45) start at cutover. The staging spine (46, then 47) gates the
static-fact specializers (57, 70–72, 75 and their followers 73, 74, 76),
Wasm materialization (38), and builtin acceleration (85). Task 25's lab gates
the existing-specializer lane (29–34) and the efficiency mechanisms (78–85).
Task 28 depends on every specializer task and on lazy compilation (67), so
phase B cannot close with one of them open. Task 61 gates only on the
mechanisms it needs.

Before phase B, a "Performance evidence" section asks for measurement only:
record the numbers and do not change code to move them. Existing
specializations (field/method caches, numeric arming, superinstruction rows)
stay as they are, frozen, and must pass task 42's optimized-versus-generic
gate at every change; they are not extended.

Phase A keeps the structural work that correctness needs: the foundation
contracts (07–10, 52, 56), stack traces (63), async-context hooks (64),
resource limits (65), differential fuzzing (66), the shared Wasm lowering and
engine entry (37, 40), and the specialization gate (42).

## Kernel watermark

The next core is layered like a metacircular Lisp: a small kernel of primitive
operations, with every other semantic defined on top of it. The plan was to
declare the watermark before porting builtins. Instead, Test262 was closed first
with every builtin inside `impl Vm`. The order is now explicit, correctness
comes first, and each family still moves only once:

1. Task 20 closes Test262 at 100%, and task 48 freezes that pass set as a fast
   ratchet. Wasm (23) and Node (22) close on the same shared VM, task 24 proves
   all three together, and task 27 deletes the legacy runtime.
2. Task 46 declares the watermark tables and splits the kernel and library
   crates on the single core, then moves the builtin families of tasks 12–17
   onto the kernel API. Every move is a ratchet diff with zero regressions
   across all three suites.
3. Task 47 derives the generating extension from the tables. Every kernel
   body that only speeds up a library operation becomes a fast path on that
   library-bodied row, and the reference kernel (no fast paths, no
   specialization) passes all three conformance inventories. What remains is
   one core: a kernel, a library defined through its API, and shared
   algorithm crates behind `AlgorithmBoundary`.

In Self, everything is a message send, and an object's map decides how it
responds. JavaScript's send is `Invoke` (`Get` then `Call`), and its selectors
are property keys and well-known symbols. The 13 internal methods, `Call` and
`Construct` included, form a complete object protocol; Proxy proves it can
emulate any object. So the internal methods are selectors, dispatched by
brand. Every object, the ordinary ones included, answers them through library
handler rows. The fundamental set is only what the protocol cannot express:

- a value algebra (tags, SameValue, primitive numeric operations);
- strings;
- storage: branded slots (internal slots, private names, and environment
  records) and shape-backed property storage;
- dispatch by brand;
- enter activation, throw, and suspend/resume;
- lifetime and agent primitives.

Everything else is library code over that set. That includes every abstract
operation, builtin, and opcode fallback, and every brand's handlers: Ordinary,
the exotic kinds, and every kind of function. Speed and memory come from the
kernel, not new semantics. Brand rows fix cell layouts. Fast paths are guarded
by fact rows and fall back exactly to their row's library body. The compiler
is the generating extension over the three tables, with budgets whose
exhaustion yields the generic residual. The reference kernel, with no fast
paths and no packed layouts, is the correctness oracle (tasks 46, 47).

| Layer | Owns | May use |
| ----- | ---- | ------- |
| 0. Kernel | fundamental row bodies, brand-derived layouts, shapes, storage, roots and GC, fact cells, fast-path bodies, activations, realms, jobs, compiler, interpreter, specializers | nothing above it |
| 1. Language library | every other operation row: spec abstract operations, every brand's internal-method handlers (Ordinary included), opcode fallbacks, all intrinsics and builtins | the kernel API only |
| 2. Algorithms | RegExp, Intl, Temporal, Date, numeric/string conversion (task 45) | the algorithm boundary trait only; no kernel or library types |

The watermark between layers 0 and 1 is a crate boundary crossed only through
three declared tables:

- **operations**: one row per kernel primitive, abstract operation, brand
  handler, opcode fallback, and builtin. Each row has a body (`kernel` or
  library), an effect set, optional fast paths, and install metadata for
  intrinsics.
- **brands**: fixed slot layouts plus internal-method overrides. These cover
  object kinds, internal slots, private names, and environment records.
- **facts**: named predicates with an owner, invalidating transitions, and a
  `Valid → Invalidated` state machine. Protectors, validity cells, and
  inline-cache shape checks are all fact rows.

Roles are derived, not stored. A row is fundamental if its body is `kernel`,
accelerated if it has fast paths, and statically evaluable if its effect set
is empty. A fast path's exact fallback is its row's library body, so the
kernel never adds semantics, and a reference kernel without fast paths must
pass the same conformance run. The kernel API is opaque and cache-friendly:
lookups return results a cache can hold, and value encoding stays private.
Every row declares its body and effect set, and every fast path its facts,
from the start, so the staging rules hold before anything is derived from
them. Kernel experiments therefore change only the kernel. A change that
forces library edits is a change to the operation or brand table and is
reviewed as one.

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
| Builtin written against spec operations | Array, Object, String, Promise, TypedArray, Proxy, collection methods | Port file by file onto the kernel API (task 46, after cutover): Get, Set, DefineOwnProperty, HasProperty, Delete, OwnPropertyKeys, Call, Construct; keep the algorithm, replace storage access |
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
3. Close Wasm and Node on the shared VM (tasks 23, 22), prove them together
   (task 24), and delete legacy at cutover (task 27).
4. Declare the watermark and the `AlgorithmBoundary` rows (task 46). Finish
   extracting next-core Temporal, Intl and Date into the shared crates (task
   45, already in progress).
5. Move the builtin families onto the kernel API (task 46), then derive the
   generating extension (task 47).

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
- On each of the eight V8-v7 fixtures (Crypto, DeltaBlue, EarleyBoyer,
  NavierStokes, RayTrace, RegExp, Richards, Splay), Quench has the highest
  median Score and the lowest median maximum RSS among Quench, QuickJS,
  Bun/JSC with its JIT disabled, and Node/V8 `--jitless` (task 61). Parity with
  the pinned `../v2` reference (task 26) is an intermediate step.
- JavaScript and Wasm execute on one VM; no crate other than the shared runtime
  executes Wasm.
- The final tree contains no guest JIT, copy-and-patch stencil, executable-memory
  runtime, legacy backend, or sibling-checkout build dependency.

## Out of scope

These Node and VM facilities are deliberately not implemented, because the
Node gate is task 19's frozen legacy passing set and no tracked fixture needs
them. Adding one requires a task, not an incidental change:

- the inspector protocol and debugger (`--inspect`, breakpoints, stepping);
- source-map support for stack traces (`--enable-source-maps`);
- CPU profiles and heap snapshots (`--cpu-prof`, `v8.writeHeapSnapshot`).

Task 63's position table and frame walk are the foundation any of them would
build on.

The generic Lisp-mindset skill suggests size caps, but this repository's rules
explicitly reject mandatory line-count or complexity ceilings. Cohesion and
reviewability remain required; no numeric cap is imposed.
