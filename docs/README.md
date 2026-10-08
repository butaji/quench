# Documentation

- [Repository rules and Lisp mindset](../AGENTS.md)
- [Active rewrite queue](../tasks/index.json)
- [Task execution contract](../tasks/README.md)
- [Pinned v2 design decisions](v2/architecture.md) (copied from `../v2` at the indexed reference commit; binding for the next core)
- [Wasm boundary](spec.md)
- [Test262 stages](STAGES.md) and [Node stages](../STAGES.md)
- [V8-v7 commands](v8_v7.md)
- [Benchmark micros](../quench-bench/micros/README.md)

Documentation records stable contracts and reproducible commands. Current work
and status live only in `tasks/index.json` and the numbered task files. Build
and measurement artifacts belong under ignored `target/` directories and must
record their source revisions and commands.

The rewrite target is an interpreter-only runtime derived from the pinned
`../v2` snapshot. Node host behavior remains in `quench-node`; JavaScript
semantics remain in Quench's shared runtime; OXC owns syntax. The final
conformance gates are every lower-stage-adjudicated pinned Test262 case, every pinned
Wasm directive, every upstream Node fixture tagged `framework-core` in
`parallel.txt`, and the pinned Express, Koa and Fastify scenarios. Direct
Test262 conflicts follow the lower-numbered-stage rule recorded in
[task 20](../tasks/20.md#current-conflict-resolution); contradictory higher-stage
outcomes remain visible as failures and never count as passes. The broader Node
inventory is diagnostic only.
The two-stage scope and completion rules live in [tasks/README.md](../tasks/README.md).

The rewrite gate uses Quench's shared runtime. Test262 progress uses affected
stages with canonical `run-stages` plus saved-report comparison for ordinary
changes. Full inventories run only at gates 24/27 and stage transitions. Every
case has its own mandatory timeout, and the runner reports and executes
deterministic batches capped at 100 cases. Task 20 owns the current stage and
cumulative coverage record.

The default `dev` profile is incremental and uses no optimization for the
shortest Rust edit/compile cycle. For conformance iterations, use the
incremental `iteration` profile: it uses level-one optimization, parallel code
generation, and no LTO so the runner remains reasonably fast without making
every edit pay release-build costs. It is not the production/performance-
evidence profile. Use `--release` for the final release gate and matched
performance measurements.

```sh
TEST262_TEST_TIMEOUT_MS=900000 TEST262_JOBS=4 \
  cargo run --profile iteration -p quench-test262 --bin run-stages -- 10 10
```

```sh
TEST262_TEST_TIMEOUT_MS=30000 TEST262_JOBS=10 \
  cargo run --release -p quench-test262 --bin run-all
```

```sh
TEST262_TEST_TIMEOUT_MS=30000 \
  cargo run --release -p quench-test262 --bin run-stages -- 0 0
```

The canonical `run-stages` and `run-all` runners use the shared VM. The
`run-test` binary retains legacy execution for now and is not evidence for the
Stage A gate. Every Test262 execution runner/tool (`run-test`, `run-all`,
`run-stages`, `triage`, and execution modes of `compare-runs`)
requires a positive `TEST262_TEST_TIMEOUT_MS`; the batch wrapper has the same
requirement. `run-all` and `run-stages` run case processes concurrently
(configurable with
`TEST262_JOBS`, defaulting to available parallelism), preserve discovery-order
reports, and keep stage batches capped at 100. The fixture comparison scripts
`tools/diff-next.mjs` and `tools/run-all-next.mjs` require positive
`DIFF_TIMEOUT_MS` values. Their differential agreement requires completed
observations on both sides: signals, timeouts, failed launches and missing
statuses fail verification even if their outputs match. Completed nonzero exits
remain comparable. The directory report derives `inventory_complete` from
its records; this is process completion, not proof that Node APIs are implemented.
The development shared-VM entry runs JavaScript with the existing Node host.
The Stage A Node gate is the `framework-core` profile selected from
`parallel.txt` and the three pinned package scenarios in `tests/frameworks`.
The broader inventory and owner audits remain diagnostic and do not add gate
cases.

Node inventory review uses implementation owners and per-case obligations in
`tasks/node-compat-inventory.json`. Run from the repository root:

```sh
cargo run --profile iteration -p quench-node-test --bin run-compat -- \
  --inventory tasks/node-compat-inventory.json --list
```

This lists the diagnostic broad Node inventory and validates every input hash,
including support files. Use `--filter NAME` for a focused diagnostic query.
It does not define the Stage A Node gate; profile membership comes from
`parallel.txt`, and package scenarios come from `tests/frameworks`. Both Node
runners use isolated child deadlines and preserve stdout/stderr separately from
case results.

`run-all` writes its full per-test report to
`target/test262-report.json` and compares a full-inventory run against
`target/test262-next-ratchet.json`. A first complete all-pass run freezes the
baseline; a later lost pass is reported as a regression and fails the command.
Set `TEST262_REPORT` or `TEST262_RATCHET` to select other paths. A
`TEST262_BATCH_SIZE` subset still writes its report but does not update or
compare the full-inventory ratchet.

For an explicitly authorized all-pass assumption, freeze expectations without
executing any cases:

```sh
TEST262_TEST_TIMEOUT_MS=30000 \
  cargo run --profile iteration -p quench-test262 --bin run-all -- \
  --freeze-expected-pass-set
```

This refuses to overwrite an existing baseline and records
`basis: user_assumption` and the suite revision, without generating observed
outcomes. A later complete observed all-pass run replaces that basis with
`observed`. `run-stages` compares only baseline expectations owned by its
selected stages and path filter, including previously expected paths removed
from discovery. Focused runs never update the baseline. `TEST262_RATCHET`
selects the same baseline in both runners; an explicitly selected missing or
invalid baseline is an error. A stage selection discovering no cases is an
error.

`run-stages` writes discovery-order case outcomes, stage counts, normalized
failure messages, and source/binary/host provenance to
`target/iteration/test262-stages-report.json` (override with `TEST262_REPORT`).
The report is written before a failed-stage verdict, so failures remain available
for triage. Compare saved reports without executing tests:

```sh
cargo run --profile iteration -p quench-test262 --bin compare-runs -- \
  --reports target/iteration/before.json target/iteration/after.json
```

A lost pass, including a missing formerly passing case, exits nonzero.
Malformed, duplicate, empty, or truncated outcome inventories and inconsistent
pass/failure counts are errors. A clean verdict requires identical case
inventories, including cases that previously failed; dropping a known failure
cannot silently pass.
Compare reports with the same selection; a narrower current selection reports
missing baseline passes as regressions.

For long runs, advance one stage at a time with `run-stages`; it divides
large stages into deterministic batches of at most 100 cases and preserves
discovery order. Reserve `run-all` for explicit whole-inventory qualification.

## Shared runtime host roots

The development `quench_runtime_next::Runtime` exposes `global_root`, `string_rooted`,
`get_property_rooted`, `set_property_rooted` and `call_rooted`. These use the
existing VM semantics and derive program context from its owned program store.
All inputs are live roots belonging to that runtime; validation precedes guest
coercion, getters or callbacks. Calls preserve callable program ownership and
restore the current realm.

`set_property_rooted(object, key, value, receiver)` uses shared `Reflect.set`
semantics. Its four roots are validated before key coercion, setters or Proxy
traps. It returns `true` for acceptance or `false` for refusal, without creating
a result root. A throw returns the same retained `RootedError` as reads/calls.
The embedder owns the policy for handling a refusal.

Successful reads and calls return a retained `RootId`. Failure returns `RootedError`, containing
the original `JsError` and an optional retained guest `exception` root;
host-validation failures have no guest exception. Release success and exception
roots explicitly after use. Retained exceptions preserve identity through
subsequent calls and collection. A fresh `execute` invalidates all earlier
roots, including strings allocated before initialization. Globals and guest
operations require initialized execution. These mechanisms do not install
Node APIs or close task 43's host migration.

Rust native bindings use `Host::functions`, a stable table of `HostFunction`
records. `host_functions!` declares global or module-owned functions and derives
their name/arity descriptors. `Runtime::host_function(HostFunctionId)` returns
a retained callable; global entries are installed during execution initialization.
The operation index stays in the existing native callable environment, and
mutable host state belongs in the host.

`Host::initialize` runs after intrinsics/native globals and before guest code.
It receives the same scoped context, allowing the embedder to install its
namespace without guest bootstrap code. `NativeContext::object_rooted` delegates
to shared object creation with the active realm's intrinsic prototype;
`RootedError::host` reports an installation/host failure without inventing a
guest exception identity.

Callbacks receive a borrowed `NativeContext`, receiver root and argument roots.
Roots created by the context, including thrown values, expire when the callback
returns. `retain` explicitly promotes a root for later host release. Calls and
property operations re-enter shared VM semantics; return values and exceptions
must be live roots from that VM. Foreign or released handles fail validation.
Typed failure metadata survives the boundary; raw guest exceptions without
a validated root are discarded.
Fresh execution invalidates promoted roots too. This mechanism currently covers
nonconstructible functions; Node classes and host migration remain task 43 work.

The development shared-VM Node entry uses the existing `NodeHost` on this VM.
CommonJS files compile through the standard wrapper; relative/package
resolution and file I/O stay in `quench-node`. The selected framework profile
and package scenarios, rather than a broad Node API inventory, define the Stage
A Node gate.

The CLI selects Module for `.mjs` and package-scoped `.js` with `type: module`;
CommonJS files and eval retain their respective scopes. Production cutover
remains task 27; task 24 owns integrated qualification of the selected Node
gate together with Test262 and Wasm.
See
[task 21 qualification](../tasks/evidence/task21-shared-commonjs-loader.json).

Run the selected upstream Node `test/parallel` profile against the shared VM
with the canonical `run-parallel` worker. Profile membership
comes from `crates/quench-node-test/node-tests/parallel.txt`; `--filter`
narrowly selects cases within that profile for affected-case checks.

```sh
cargo run --profile iteration -p quench-node-test --bin run-parallel -- \
  --profile framework-core --filter test-path-join.js
```

## Shared Wasm scopes

Build and run the separately named shared-VM development target on explicit
files or directories. Ordinary changes select affected files; a directory
selection includes every `.wast` recursively, including proposals. The positive
`WASM_FILE_TIMEOUT_MS` deadline applies to each script worker, preserving its
ordered module lifecycle. Do not count a crash, timeout, parse failure or empty
discovery as a pass; `inventory_complete: false` rejects qualification.

```sh
WASM_FILE_TIMEOUT_MS=60000 \
  cargo run --profile iteration -p quench-wasm-test --bin run -- \
  --report target/iteration/wasm-shared.json \
  crates/quench-wasm-test/testsuite/i32.wast
```

Reports preserve every parsed directive with a file/ordinal identity, expected
and actual results, input hashes, binary hash, command and worker outcomes.
The `next` report schema is shared with saved Test262 outcome comparisons:

```sh
target/debug/compare-runs --reports \
  target/iteration/wasm-before.json target/iteration/wasm-shared.json
```

Also require identical inventories and pinned input hashes for the compared
scope. Freeze the runner executable before qualification; changing it during a
run invalidates the report. Typed Wasm invocations preserve the runtime heap and host roots; module
exports share one residual program. Scalar-global modules use rooted
`Runtime::instantiate_wasm_module` instances, including global-only modules;
`instantiate_wasm` also accepts an existing typed function view. Use
`invoke_wasm_values` for complete typed result vectors; `invoke_wasm` and
`execute_wasm` remain scalar/void boundaries and reject multi-result entries
before guest effects. `execute_wasm_values` supports unbound typed functions.
Use `wasm_global` for exported state projection, and `release_wasm` to release
the instance root. Memory32/Memory64 and custom pages use rooted instances and
one heap memory owner holding bytes and the validated MemoryType. Shared and
unshared memories use one synchronized backing store; imported sharedness must
match exactly, and shared declarations require a maximum. Reads, writes, growth
and overlapping copies preserve the original backing identity. Two-memory copies
acquire backing locks in a consistent order. This admits shared memory declarations
and imports. Integer atomic load/store and RMW operators reuse scalar byte and
integer rules, including narrow wrapping and natural-alignment traps. One backing
lock spans each RMW, and a shared order covers atomics, size/growth and fences.
Residual format71 admits the atomic access window, wait/notify forms and fence; selector-specific
arity is validated before effects and every window input participates in root
inference. Wait32/wait64 comparison and registration share the backing lock;
waiter state controls completion, and native park/unpark permits prevent missed
notifications. Notify is address-based across both widths; zero/finite/infinite
signed nanosecond timeouts retain exact integer semantics. Completion releases
registrations and empty queue storage. Cross-agent embedding, JavaScript memory
views and JS Atomics interoperability remain open. Index width,
page size and limits derive from that type. Effective addresses use checked u64
arithmetic and raw scalar storage; full-width offsets live in the constant pool.
Residual format53 rejects the earlier Memory32 address operands. Data
segments retain immutable buffer owners. Scalar access, size/growth, bulk copy/fill and
memory initialization use shared dispatch. Active-data bounds are checked at
instantiation; active segments drop after initialization, while passive availability
is independent per instance and `data.drop` is idempotent. Table32/Table64 use
typed heap-owned reference vectors; get/set, size/grow, fill and overlapping/
cross-table copy share dispatch and root tracing. The table cell owns its index
width; indices retain unsigned I32/I64 bits until checked host range conversion.
Init uses I32 segment input/length and the destination width; mixed copy uses
each table's index width and the smaller length width. Active element offsets
validate against their destination table. Growth returns the old size or failure
in the table's index type and preserves state on failure. Nullable function/external values use VM
Values, and function references share existing closure identity per instance.
`WasmValue::V128` transports exact 128-bit payloads through the typed Runtime
and Rust host interfaces, globals and multi-result calls. Constants and heap
cells store little-endian bytes; SIMD logical/shuffle/swizzle/splat and scalar
lane extraction/replacement share macro-derived operator facts and dispatch.
Full-width and specialized vector memory forms use existing memory range checks.
Splat/widen/zero loads project checked bytes through SIMD rules; lane loads/stores
compose scalar memory access with lane replacement/extraction.
F32x4/F64x2 arithmetic, comparison masks and rounding project shared scalar float
rules over lanes. Pseudo-min/max compare numerically and select original operand
bits; ties/unordered comparisons retain the first operand. Standard integer
SIMD projects scalar wrapping/comparison rules over lanes;
SIMD saturation, widening/narrowing, reductions and pair/dot/Q15 grouping are
pure lane rules. Standard SIMD conversions reuse scalar conversion signatures
and semantics; active lanes derive from source/result widths, with zero upper
lanes for narrowing. Relaxed SIMD chooses deterministic spec-admitted outputs through existing
operations and named ternary compositions. Madd/nmadd use unfused arithmetic;
lane selection uses bitselect; byte-pair dot uses signed saturation. The residual format admits vector constants/operators and rejects older
encodings; `ResidualProgram::FORMAT_VERSION` owns its current version. Direct JS V128 arguments/results are not supported.
Wide arithmetic returns two ordinary I64 slots through the multi-result ABI.
Add/sub compose wrapping operations with carry/borrow; signed/unsigned high
multiply rules share the existing scalar operator table. No wider value payload
or second executor is introduced.
Root a returned heap reference with `Runtime::root` before subsequent VM work
and retain that root while using it. Active elements initialize tables in
module order and then drop; passive references have independent instance
availability, and declarative segments drop at their declaration-order transition. elem.drop shares the
binding transition used by data.drop. Indirect targets compare structural
signatures from each program's canonical pool before the ordinary VM call.
Start functions execute once per instantiation after active data/elements and
before the instance is returned. A start trap releases the provisional instance
root. Unbound execution rejects modules carrying a start effect; use the
instance API for those modules.
Shared-suite workers reserve `quench_stack::WORKER_STACK_SIZE`; worker failures
or missing directives invalidate completion. Table definitions carry a
`WasmReferenceInitializer` shared with element items: null, function index or
a typed constant expression.
`WasmTable` owns type/limits and a `WasmTableInitializer`: a reference initializer
or an import name. A `TableType` converts to the implicit-null reference form. Function initializers use the rooted instance and existing
closure identity, before active elements and start. Supported func/extern table
types retain nullability; copy/init use the same subtype operation.
Memory/table imports use `instantiate_wasm_module_with_imports` with rooted handles
in `module.imports()` declaration order; each import name owns its validated index.
`wasm_memory` and `wasm_table` project the original heap identity;
root that Value before further VM work. The importing instance retains it, so
release temporary import roots after instantiation. Table element types and index widths match exactly; limits use current size and original declared
maximum, including re-exports. Memory owners preserve the difference between
an absent maximum and an explicit bound; growth derives the index/page address cap.
Memory imports match index width and actual page size, current size and original
maximum. Memory init uses I32 data input/length and the destination width; mixed
copy uses each memory width and the smaller length width. Active data offsets
validate against the destination memory. Size/grow results use its index type.
Valid maximal declarations are distinct from host allocation representability.
`JsError::wasm_link_error` distinguishes linking failures from guest traps.
Active elements execute before active data and start, with bounds checked per
segment; earlier writes to imported resources survive a later initialization trap.
The core [instantiation contract](https://webassembly.github.io/spec/core/exec/modules.html)
owns this ordering, verified against local Node. Segment contents exist before
initialization effects; escaped functions can access segments whose transition
was not reached. The core contract retains future active data until its drop;
Node v26.10.0 eagerly drops active data, as confirmed in
[V8 initialization](https://raw.githubusercontent.com/v8/v8/main/src/wasm/wasm-objects.cc).
The recorded oracle difference does not change pinned suite expectations.
Global imports/reexports retain the original traced cell, including its type,
mutability and live value. Immutable imported `global.get` initializers allocate
an independent global initialized from that value. The shared get/set operations
follow the [core global contract](https://webassembly.github.io/spec/core/exec/instructions.html#exec-global-get).
`Runtime::wasm_global` reads its typed value; `Runtime::wasm_global_binding`
projects the identity to root for imports. Mixed global/memory/table declaration
order derives from one import-name projection
([qualification](../tasks/evidence/task37-wasm-global-imports.json)).
Wasm function imports and re-exports retain the original function cell and its
captured instance. Canonical signatures distinguish source function indices
from residual body indices; imports generate no forwarding bodies. Direct
imported calls, starts and function references use the same shared ABI and
[instantiation contract](https://webassembly.github.io/spec/core/exec/modules.html#exec-instantiation).
`Runtime::wasm_function` projects that identity to root for imports
([qualification](../tasks/evidence/task37-wasm-function-imports.json)).
Exact-function imports retain exactness in the canonical import fact. Linking
checks the original dynamic callable declaration for equivalence, including
through inexact re-exports; ordinary imports admit subtypes. Static `ref.func`
projections are exact only for definitions and exact imports. Initializer
admission uses that same reference subtype operation
([proposal](https://github.com/WebAssembly/custom-descriptors/blob/main/proposals/custom-descriptors/Overview.md),
[qualification](../tasks/evidence/task37-wasm-exact-imports.json)).
Normal and tail reference calls share the null-function trap selector; explicit
`ref.as_non_null` retains its generic null-reference contract
([qualification](../tasks/evidence/task37-wasm-null-calls.json)).
Exception tag declarations allocate distinct identities in the shared heap;
imports/re-exports retain the original cell. Tag linking compares exact closed
declaration types, including recursive-group identity, before initialization
effects. Root exported handles before subsequent VM work and release import
roots after instantiation. `Runtime::wasm_tag` projects that identity. Tag
bindings precede the final imported-function suffix in the instance layout
([core allocation](https://webassembly.github.io/spec/core/exec/modules.html#alloc-tag),
[qualification](../tasks/evidence/task37-wasm-tags.json)).
Modern `try_table`, `throw` and `throw_ref` use one traced exception cell owning
the original tag and payload. Static regions derive catch trampolines and existing
activation handlers; typed error transport preserves reference identity across
rethrows. Traps bypass catches; null exception references raise a typed trap.
Root transported exception references before further VM work. Start exceptions
remain distinct from link errors and traps in the suite adapter
([qualification](../tasks/evidence/task37-wasm-exceptions.json)).
Legacy `try`, `catch`, `catch_all` and lexical `rethrow` lower to the same
exception operations. Only the try body is protected; exceptions from catch arms
propagate outward, while rethrow retains the original caught reference. Modern
and legacy lowering share tag matching and payload projection
([qualification](../tasks/evidence/task37-wasm-legacy-catches.json)).
Legacy `delegate` resolves outer labels during lowering. PC-bounded handlers
write the original exception to a destination frame slot and jump to its lexical
rethrow continuation. Normal results bypass that continuation, branch exits end
protection, and tail transfer discards the replaced frame's handlers. Forward
jump facts remain lowering-only; runtime handler selection is unchanged
([qualification](../tasks/evidence/task37-wasm-delegate.json)). Shared tags and
JavaScript/foreign-host exception embedding remain unfinished.
Custom descriptor constructors share the regular struct construction model.
The original type graph derives exact descriptor types; the existing GC cell
owns and traces the optional descriptor. Allocation in bodies and constant
expressions preserves its original identity. Complete operand windows include
the final descriptor input. `ref.get_desc` projects that same reference;
null descriptors trap separately from null source references. Initializer traps
retain typed transport. Format68 admits the appended operations without changing
packed operand widths
([qualification](../tasks/evidence/task37-wasm-descriptors.json)).
Descriptor casts and branch-casts share one stored-identity predicate. Descriptor
input types derive from target exactness and the original graph; null descriptors
trap before nullable-source admission. Branches carry the original reference and
other branch payloads. Module admission rejects missing descriptor relationships
before execution. Format69 admits the cast/test operations without new cells or
allocation; JavaScript prototype reflection remains unfinished
([qualification](../tasks/evidence/task37-wasm-descriptor-casts.json)).
Initializer expressions retain one admitted representation: known values fold
inline, unresolved immutable globals remain symbolic. Global initializers,
active segment offsets and nullable func/extern reference initializers evaluate
against actual instance bindings through the same scalar operations; no imported
value is substituted during lowering
([qualification](../tasks/evidence/task37-wasm-initializer-expressions.json)).
`ref.func` is a symbolic dependency in that same expression model. Instantiation
roots its environment before installing defined globals in declaration order;
then table/segment effects and start execution proceed. Function references use
the original function cache/import identity and captured environment. No
placeholder globals or forwarding functions are constructed. Frontend reference
initializers share expression lowering
([qualification](../tasks/evidence/task37-wasm-function-initializers.json)).
`Runtime::wasm_host_function` creates a rooted typed native callable. Its native
environment owns the immutable signature and embedding operation ID. Linking,
function-reference validation and indirect calls use one signature projection;
there is no host function registry or forwarding body. `Host::call_wasm` receives
bit-exact `WasmHostValue` scalars and borrowed generation-checked reference roots.
Those temporary handles expire after return; result count, type and root ownership
are checked before transport through the shared ABI. Persistent caller-owned
roots remain caller-owned. `Runtime::invoke_wasm_host_function` and imported calls
use the same guarded native boundary and result decoder. Host creation initializes
the VM before publishing its first root. The shared Wast provider installs
spectest signatures explicitly and records real print effects
([qualification](../tasks/evidence/task37-wasm-host-functions.json)).
Original declaration graphs own concrete/exact function and GC reference types
across callable, global and table boundaries. Unshared i31 values, struct objects
and arrays use the same Value/root transport; tests, casts, cast branches and
reference equality use existing type predicates and label transfers.
Struct constructors, field access/mutation and instance-owned constant construction
share storage semantics. Constructor operands expose their complete register windows to shared
bounds validation and GC liveness. Array constructors support repeated/default/fixed
contents, length and element reads/writes. Packed reads and mutation share struct
storage semantics; indices are unsigned i32 and null/out-of-bounds access traps
before mutation. Array fill/copy share storage and graph-aware source covariance;
complete ranges are checked before writes, self-copy preserves overlap and distinct
arrays copy without temporary storage. Array repeated/default/fixed constant constructors allocate through the
same storage operation at instantiation; independent instances receive fresh
objects, and allocation traps retain their typed identity. Data/element-backed construction and initialization reuse existing segment
bindings and dropped-state transitions. Data uses shared little-endian loads;
elements retain references. Complete source/destination ranges and reference
admission are checked before mutation, including empty ranges. Each dense array payload has a 64 MiB implementation resource limit,
owned by `Vm::check_wasm_array_size`, to bound committed writes from unsigned guest lengths.
Excess length or reservation failure traps before publishing an array.
External/internal reference conversions use one operation in instructions and
constant initializers. GC objects retain identity; i31 values use the shared
signed projection, and opaque host payloads use one traced cell outside eqref.
Wast host tokens have rooted opaque identities rather than numeric surrogates.
JS/Wasm callable adapters, embedding re-entry, JavaScript descriptor facades,
shared globals/tables, JS memory/Atomics integration, foreign host exceptions and remaining
proposals still need implementation or integration.
Explicit JavaScript `Runtime::execute`
retains its fresh-execution reset contract. Unsupported shared operations
are failures, never legacy fallbacks or skips. The legacy `run` target is only a pre-cutover
reference; task 27 removes it. Module state/imports and remaining operators
still need the shared implementation and complete suite proof recorded in tasks.
