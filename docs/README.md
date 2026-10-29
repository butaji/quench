# Documentation

- [Repository rules and Lisp mindset](../AGENTS.md)
- [Backlog, status and lanes (Linear)](https://linear.app/todoxapp/project/quench-abb744c79c4e/overview)
- [Measurement evidence](../evidence/)
- [Pinned v2 design decisions](v2/architecture.md) (copied from `../v2` at the indexed reference commit; binding for the next core)
- [Wasm boundary](spec.md)
- [Test262 stages](STAGES.md) and [Node stages](../STAGES.md)
- [Native Node host contracts](node-host-contracts.md)
- [Shared array operation contracts](shared-array-contracts.md)
- [V8-v7 commands](v8_v7.md)
- [GC verification, memory policy and experiment strategy](v2/gc-verification.md)
- [JetStream 3 commands](jetstream3.md)
- [Benchmark micros](../quench-bench/micros/README.md)

Documentation records stable contracts and reproducible commands. Current work
and status live only in the Linear project. Build
and measurement artifacts belong under ignored `target/` directories and must
record their source revisions and commands.

The rewrite target is an interpreter-only runtime derived from the pinned
`../v2` snapshot. Node host behavior remains in `quench-node`; JavaScript
semantics remain in Quench's shared runtime; OXC owns syntax. The final
conformance gates are every Node-adjudicated pinned Test262 case, every pinned
Wasm directive, every upstream Node fixture tagged `framework-core` in
`parallel.txt`, and the pinned Express, Koa and Fastify scenarios. Direct
Test262 conflicts follow the local Node-oracle rule recorded in
the task 20 resolution (git history: `tasks/20.md`); contradicted raw outcomes
remain visible as explicit waivers and never count as passes. The waivers'
single authority is `crates/quench-test262/node-waivers.json` (see
[Node waivers](#test262-node-waivers)). The broader Node inventory is
diagnostic only.
The two-stage scope and completion rules live in the [Linear project](https://linear.app/todoxapp/project/quench-abb744c79c4e/overview).

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

Production and benchmark binaries are profile-guided: `tools/pgo-build.sh
[PROFILE]` (default `production`) builds an instrumented `quench-node`, trains it
on the interpreter curriculum (`quench-bench/deegen-curriculum`) and the Node
compatibility fixtures (`tests/node-compat`), then rebuilds `PROFILE` with the
merged profile into `target/PROFILE/quench-node`. The scored engine suites are
never part of the training corpus. It needs the toolchain's `llvm-tools`
component. Measured on x86_64 Linux against the plain production build, V8-v7
Score rose 7-24% on six fixtures (Splay and NavierStokes flat) and peak RSS fell
about 1 MiB on all eight.

`tools/pgo-build.sh --static [PROFILE]` builds the static x86_64 Linux binary the
same way: both builds add `-Ctarget-feature=+crt-static -Crelocation-model=static`
for the host triple, and the binary lands at `target/<host>/PROFILE/quench-node`.
That binary can be laid out with `tools/bolt-layout.sh --pgo PROFDATA` (below) for a
Score column. It is not the memory qualification binary: PGO's inlining grows the hot
text (3.45 -> 4.60 MB), and its MemoryFirst max RSS failed the V8-v7 target on Crypto and
RayTrace pinned (TOD-197), so the memory qualification binary is the non-PGO layout,
`tools/bolt-layout.sh production`, with the repository corpus it records in
`target/layout-corpus/MANIFEST` (the Wasm ESM corpus laid out `--late`, no npm packages).
A PGO layout becomes the qualification binary only once it passes both columns. Every
mode keeps the repository's
`.cargo/config.toml` rustflags (`-D warnings`, the function alignment): the script
appends its flags with `--config`, which cargo concatenates with the configured
array. It refuses to run with `RUSTFLAGS` set, as that variable replaces both.

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

The canonical `run-stages`, `run-all`, and `run-test` runners use the shared VM.
Every Test262 execution runner/tool (`run-test`, `run-all`,
`run-stages`, `triage`, and execution modes of `compare-runs`)
requires a positive `TEST262_TEST_TIMEOUT_MS`; the batch wrapper has the same
requirement. `run-all` and `run-stages` run case processes concurrently
(configurable with
`TEST262_JOBS`, defaulting to available parallelism), preserve discovery-order
reports, and keep stage batches capped at 100. The fixture comparison scripts
`tools/diff.mjs` and `tools/run-all.mjs` require positive
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
`evidence/node-compat-inventory.json`. Run from the repository root:

```sh
cargo run --profile iteration -p quench-node-test --bin run-compat -- \
  --inventory evidence/node-compat-inventory.json --list
```

This lists the diagnostic broad Node inventory and validates every input hash,
including support files. Use `--filter NAME` for a focused diagnostic query.
It does not define the Stage A Node gate; profile membership comes from
`parallel.txt`, and package scenarios come from `tests/frameworks`. Both Node
runners use isolated child deadlines and preserve stdout/stderr separately from
case results.

<a id="test262-node-waivers"></a>
Node waivers list, by group and reason, each pinned case whose raw Quench
failure the local Node oracle reproduces, with that exact failure. The runners
compile the manifest in. A waived case keeps its raw outcome in every report,
tagged with its `waiver` group and counted in `waived`; the runners accept it
only while it fails with exactly the recorded failure. A waived case that
passes or fails differently fails the run until the manifest is updated. A
case the oracle cannot run (a module case) is waived only by its group's
Node-arbitrated rule and is marked `"node": "unverifiable"`.
Re-adjudicate the manifest, or triage other cases, against the installed Node:

```sh
node tools/test262-node-oracle.cjs
node tools/test262-node-oracle.cjs built-ins/Error/prototype/stack/prop-desc.js
```

`run-all` writes its full per-test report to
`target/test262-report.json` and compares a full-inventory run against
`target/test262-quench-ratchet.json`. A first complete run in which every case
passes or fails as its waiver records freezes the observed passes as the
baseline; a later lost pass is reported as a regression and fails the command.
Set `TEST262_REPORT` or `TEST262_RATCHET` to select other paths. A
`TEST262_BATCH_SIZE` subset still writes its report but does not update or
compare the full-inventory ratchet.

Compare engines on both suites, case by case, with `quench-bench`'s
`conformance-compare` (Quench, Node and Bun; wall, CPU and peak RSS per process,
aggregated per area against a 20% margin). Node and Bun run Test262 through
`tools/test262-engine-case.cjs` and Wasm scripts that `wast-json` converts through
`tools/wast-engine-run.cjs`; see
[the 2026-10-22 evidence](../evidence/conformance-engines-linux-2026-10-22/README.md)
for the commands, engine switches and caveats.

`run-all --jit` and `run-test --jit` run every case in compiled execution
(`ExecutionMode::Compiled`, the mode `quench-node --jit` uses), as the Wasm
runner's `run --jit` does for the Wasm testsuite. Both suites must pass in both
modes. A compiled run writes `target/test262-report-jit.json` (its provenance
names `execution_mode`) and is held to the same ratchet and waiver manifest as
an interpreted run: one expected pass set for both modes.

For an explicitly authorized all-pass assumption, freeze expectations without
executing any cases:

```sh
TEST262_TEST_TIMEOUT_MS=30000 \
  cargo run --profile iteration -p quench-test262 --bin run-all -- \
  --freeze-expected-pass-set
```

This refuses to overwrite an existing baseline and records
`basis: user_assumption` and the suite revision, without generating observed
outcomes. A later complete observed run without rejected outcomes replaces
that basis with `observed`. `run-stages` compares only baseline expectations owned by its
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

## Compile on first call

A program compiles whole at load, which decides every function's layout, constants and
sites and reports every early error. A function that can compile again from its own source
then keeps only its header (`FunctionHeader`: the facts closure creation, `IsConstructor`,
brand checks and `toString` read) and compiles again on its first call
(`ResidualProgram::function`). Replayable: function declarations and expressions, arrows,
object literal methods and class methods, getters and setters (not constructors, field
initializers, static blocks or decorated methods) of ordinary scripts and modules, that
resolve every name statically, are reached by no dynamic lookup, resolve no private names,
and are not called where they are made (an IIFE or a host wrapper). A replayable function
inside no function that may give it binding sites (direct eval, `with`, `delete` of a name)
finishes and releases its record as soon as it compiles; the others release theirs when the
program is stored. Host bodies whose programs keep only static text, and the main
CommonJS entry, whose functions run at once, stay compiled.

A replay parses the function's text alone in a wrapper that gives it its syntactic context,
and emits it against what the load recorded (`compile/replay.rs`): table cursors, the
bindings its scopes provided, its layout inputs and its `arguments` slot. It adds no table
entry. The `verify-replay` feature checks that each replay equals its load record; run
Test262 with it and read the log:

```sh
cargo build --release -p quench-test262 --features verify-replay
QUENCH_REPLAY_LOG=target/replay.log TEST262_TEST_TIMEOUT_MS=30000 \
  target/release/run-all
```

Each program writes a `replay-summary` line; `replay-differs` and `replay-failed` lines name
a function whose replay does not equal its record or does not compile.

Lazy code generation (TOD-196) needs each function's captures of bindings outside it before
its code exists. A `verify-replay` build with `QUENCH_LAZY_FACTS_LOG` set predicts them for every
plain function of a replayable source from its free names (`compile/free_names.rs`, from the
semantic analysis) resolved through the scopes it closes over, and writes one `lazy-facts` line
per function: `same`, `superset` or `missing` against what its compiled subtree captured.

## Host compile cache

`quench-node` keeps the compiled programs of Node's own library modules (the embedded `lib/`
text) and of its own host scripts across runs, so a process decodes them instead of
compiling them again. The runtime asks its host for a kept program before compiling one
(`Host::kept_compiled_program`) and offers it the compiled program's compact form after
(`Host::keep_compiled_program`). That form names each atom by its text and leaves out the
source; a later run decodes it under its own VM's atom ids and the text it would have
compiled. Atom-sorted binding lists are sorted again after decoding. A program naming an
atom that has no lossless text (a lone surrogate) is not kept. Host scripts are cached only
outside a direct eval, where their text alone decides what they compile to.

Entries live in `$QUENCH_COMPILE_CACHE`, else `$XDG_CACHE_HOME/quench/compile`, else
`~/.cache/quench/compile`; an empty `QUENCH_COMPILE_CACHE` turns the cache off. An entry is
keyed by the executable that wrote it (path, size, modification time and runtime ABI), the
module's name and the exact text compiled, and is written to a temporary file renamed into
place. An entry that is missing, unreadable or does not decode costs only the compilation it
would have saved.

A compilation parses function bodies lazily: the parser releases the tree of each outermost
plain function body, a pass before code generation checks and analyzes each such body parsed
alone, and code generation parses each function again where it compiles it, so the whole
program's tree never exists at once (cold `import typescript`: 101 -> 68 MB max RSS). A
program that does not compile that way compiles eagerly and reports the eager diagnostics.
`QUENCH_EAGER_FUNCTION_BODIES` (any value) compiles eagerly, for measuring the two. CommonJS
goal detection checks syntax the same way (`Engine::syntax_valid`).

A compilation whose program the host keeps (`ReleasedRecords::Encoded`) encodes the record of
each function it finishes and releases during its load as it releases it, lowered as its
first call would lower it (`binary::LoadEncoding`); the cache entry extends that region of
records instead of compiling every released function again only to write it. Whether names
resolve statically in a function also reads the functions around it, which finish after it,
so the entry settles that one fact when it is written. A module is encoded before its store
releases the records it kept. Its warm decode is unchanged: the records are those a replay
writes, under compact atom ids numbered in another order.

A decoded program parses nothing, so it reaches none of a compilation's phase boundaries
(`set_compile_phase_boundary`), where the host releases its read-only pages. The runtime calls
`Host::cached_program_decoded` after each decode instead. Once bootstrap has finished,
`quench-node` releases there: without it, a run whose programs all decode keeps the loader's,
decoder's and bootstrap's pages resident until its first collection. Bootstrap's own programs
decode back to back without that release, which there only refaulted the same loader code. Package modules (`node_modules`) still compile on every run, so their
functions keep compiling again on first call.

## Startup snapshot

`quench-node` restores a process's bootstrapped realm from a startup snapshot instead of
running the bootstrap: Node's bootstrap up to the point before the process's options,
preloads and entry are installed, with the builtins nearly every program loads already
loaded (`PRELOADED_BUILTINS`: events, buffer, util, path, fs, url, timers, querystring,
async_hooks, diagnostics_channel, stream, module, child_process and http), as Node's own
startup snapshot holds them. The runtime writes the VM (`Runtime::build_snapshot`) with
every identity space at its indices, so values keep their bits, and the host adds the state
the heap is bound to (`NodeHost::write_snapshot`); a field of either that is neither written
nor the restoring process's own must be empty at the snapshot point, or the snapshot is
refused. A restored host continues the bootstrap (`Host::resume`) after replacing what the
snapshot holds of the builder's process: `process.argv`, `execArgv`, `execPath`, `argv0`,
`pid`, `ppid`, the process streams' `isTTY`, the performance time origins and the
`Math.random` seed. While a snapshot is built, reading `process.env`, `process.cwd()` or
`perf_hooks` fails the build, since the snapshot would keep the builder's.

A process may use a snapshot when it runs a file or inline (`-e`, `-p`) entry on the main
thread with no other CLI options and no IPC channel (`NODE_CHANNEL_FD`). By default an inline
entry always does, since it boots in full without one, and a file does when it loads modules:
the scan below finds a builtin, a dynamic `import()`, `require`, `getBuiltinModule`, or
something it cannot follow. A file that names no module, or only `console`, boots without
one, since a fresh boot loads only what the script uses
(`SnapshotUse`, `startup_snapshot.rs`). `QUENCH_STARTUP_SNAPSHOT=on` restores one for every
eligible process and `off` for none. Restoring decodes all a snapshot holds (heap,
programs, shapes and atoms: about 5 MB resident for the preloaded builtins), so there are
two kinds (`SnapshotKind`): `preloaded`, as above, and `bootstrap`, without the builtins. A
process restores `bootstrap` when its entry's static graph cannot reach a builtin
(`startup_snapshot/entry_graph.rs`): the scan resolves static imports, re-exports and a
WebAssembly module's imports as the ES module loader does, through packages, and stops at a
builtin specifier, a dynamic `import()`, the text `require` or `getBuiltinModule`, a `.node`
addon, a file it cannot resolve or parse, or past 64 files or 1 MiB. Either kind runs a
program the same way: the kind decides only which builtins are loaded before it starts.
Snapshots live in a `snapshot` directory beside the compile cache's, one directory per
build, keyed by the library root and the variables the bootstrap reads (`HOME`,
`NODE_OPTIONS`, `NODE_PENDING_DEPRECATION`, `NODE_DEBUG`, `NODE_PATH`, `QUENCH_PLATFORM`),
with the kind in the file name. When an eligible process finds none it starts the
executable with `QUENCH_BUILD_STARTUP_SNAPSHOT=<file>`, detached and holding a lock file, which
builds it for later runs and removes other builds' snapshots. A snapshot that does not
restore boots the process as if there were none. `QUENCH_STARTUP_SNAPSHOT=trace` also
reports on stderr whether one was restored, and which kind.

## Shared runtime host roots

The development `quench_runtime::Runtime` exposes `global_root`, `string_rooted`,
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
[task 21 qualification](../evidence/task21-shared-commonjs-loader.json).

For local Node oracle checks, put a Node executable matching the pinned
checkout's major version (`tests/node/src/node_version.h`) on `PATH`. The current
checkout targets Node 26 (26.12.0); other majors can differ on API contracts such as
`util.parseEnv` object prototypes. Run the CLI/API oracle checks with
`cargo test --profile iteration -p quench-node-test --test child_entry`.

The checks compare against a Node built with its own native libraries; a host without
them keeps Quench's portable fallbacks, which several checks then tell apart (legacy
`TextDecoder` encodings and descriptors, zlib and Brotli streams, OpenSSL 3.4+ digests
such as `sha256-192`). Build the matching libraries from the pinned checkout and point the
build at them:

```sh
python3 tools/build-node-icu.py --node-source tests/node --prefix $DEPS/icu
python3 tools/build-node-zlib.py --node-source tests/node --prefix $DEPS/zlib
python3 tools/build-node-brotli.py --node-source tests/node --prefix $DEPS/brotli
(mkdir -p $DEPS/openssl-build && cd $DEPS/openssl-build && \
  $REPO/tests/node/deps/openssl/openssl/Configure --prefix=$DEPS/openssl --libdir=lib \
  no-shared no-tests linux-x86_64 && make -j4 && make install_sw)
export PKG_CONFIG_PATH=$DEPS/icu/lib/pkgconfig:$DEPS/zlib/lib/pkgconfig:$DEPS/brotli/lib/pkgconfig
# The target-prefixed names override `.cargo/config.toml`'s system OpenSSL.
export X86_64_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR=$DEPS/openssl/lib \
  X86_64_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR=$DEPS/openssl/include \
  X86_64_UNKNOWN_LINUX_GNU_OPENSSL_LIBS=ssl:crypto
```

`tcp_reuse_port_shares_os_listeners_across_processes_like_node` also needs an IPv6
interface (it listens on `::`).

Run the selected upstream Node `test/parallel` profile against the shared VM
with the canonical `run-parallel` worker. Profile membership
comes from `crates/quench-node-test/node-tests/parallel.txt`; `--filter`
narrowly selects cases within that profile for affected-case checks.

```sh
cargo run --profile iteration -p quench-node-test --bin run-parallel -- \
  --profile framework-core --filter test-path-join.js
```

For the full inventory, use an unchanged Node checkout outside any enclosing
JavaScript package. Otherwise a parent `package.json` can change the module
type of upstream child scripts. Workers run from that checkout's root, matching
the upstream harness; production module resolution uses ordinary package scopes.
The runner supplies that checkout as `QUENCH_NODE_LIBRARY_ROOT` for Node library
assets. The host resolves this installation root once at startup, so guest cwd
changes cannot move its internal-module lookup. Direct CLI users can set this
variable to an installation containing `lib/` and `deps/`; its default is the
`node/` directory beside the executable. Node library sources use their builtin
CommonJS wrapper contract independently of enclosing application package scopes.

```sh
NODE_FIXTURE_WORKTREE=$(mktemp -d)
git -C tests/node worktree add --detach "$NODE_FIXTURE_WORKTREE" HEAD
cargo build --profile iteration -p quench-node -p quench-node-test \
  --bin quench-node --bin run-parallel
target/iteration/run-parallel --all \
  --parallel-dir "$NODE_FIXTURE_WORKTREE/test/parallel" \
  --jobs 4 --timeout-secs 10 --results evidence/node-parallel.json
```

The report records the selected fixture revision and inventory hash at launch.
Cases print as they complete. Every worker's process group is stopped after
completion, failure, or timeout; Linux workers also reap orphaned children.

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
The canonical report schema is shared with saved Test262 outcome comparisons:

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
([qualification](../evidence/task37-wasm-global-imports.json)).
Wasm function imports and re-exports retain the original function cell and its
captured instance. Canonical signatures distinguish source function indices
from residual body indices; imports generate no forwarding bodies. Direct
imported calls, starts and function references use the same shared ABI and
[instantiation contract](https://webassembly.github.io/spec/core/exec/modules.html#exec-instantiation).
`Runtime::wasm_function` projects that identity to root for imports
([qualification](../evidence/task37-wasm-function-imports.json)).
Exact-function imports retain exactness in the canonical import fact. Linking
checks the original dynamic callable declaration for equivalence, including
through inexact re-exports; ordinary imports admit subtypes. Static `ref.func`
projections are exact only for definitions and exact imports. Initializer
admission uses that same reference subtype operation
([proposal](https://github.com/WebAssembly/custom-descriptors/blob/main/proposals/custom-descriptors/Overview.md),
[qualification](../evidence/task37-wasm-exact-imports.json)).
Normal and tail reference calls share the null-function trap selector; explicit
`ref.as_non_null` retains its generic null-reference contract
([qualification](../evidence/task37-wasm-null-calls.json)).
Exception tag declarations allocate distinct identities in the shared heap;
imports/re-exports retain the original cell. Tag linking compares exact closed
declaration types, including recursive-group identity, before initialization
effects. Root exported handles before subsequent VM work and release import
roots after instantiation. `Runtime::wasm_tag` projects that identity. Tag
bindings precede the final imported-function suffix in the instance layout
([core allocation](https://webassembly.github.io/spec/core/exec/modules.html#alloc-tag),
[qualification](../evidence/task37-wasm-tags.json)).
Modern `try_table`, `throw` and `throw_ref` use one traced exception cell owning
the original tag and payload. Static regions derive catch trampolines and existing
activation handlers; typed error transport preserves reference identity across
rethrows. Traps bypass catches; null exception references raise a typed trap.
Root transported exception references before further VM work. Start exceptions
remain distinct from link errors and traps in the suite adapter
([qualification](../evidence/task37-wasm-exceptions.json)).
Legacy `try`, `catch`, `catch_all` and lexical `rethrow` lower to the same
exception operations. Only the try body is protected; exceptions from catch arms
propagate outward, while rethrow retains the original caught reference. Modern
and legacy lowering share tag matching and payload projection
([qualification](../evidence/task37-wasm-legacy-catches.json)).
Legacy `delegate` resolves outer labels during lowering. PC-bounded handlers
write the original exception to a destination frame slot and jump to its lexical
rethrow continuation. Normal results bypass that continuation, branch exits end
protection, and tail transfer discards the replaced frame's handlers. Forward
jump facts remain lowering-only; runtime handler selection is unchanged
([qualification](../evidence/task37-wasm-delegate.json)). Shared tags and
JavaScript/foreign-host exception embedding remain unfinished.
Custom descriptor constructors share the regular struct construction model.
The original type graph derives exact descriptor types; the existing GC cell
owns and traces the optional descriptor. Allocation in bodies and constant
expressions preserves its original identity. Complete operand windows include
the final descriptor input. `ref.get_desc` projects that same reference;
null descriptors trap separately from null source references. Initializer traps
retain typed transport. Format68 admits the appended operations without changing
packed operand widths
([qualification](../evidence/task37-wasm-descriptors.json)).
Descriptor casts and branch-casts share one stored-identity predicate. Descriptor
input types derive from target exactness and the original graph; null descriptors
trap before nullable-source admission. Branches carry the original reference and
other branch payloads. Module admission rejects missing descriptor relationships
before execution. Format69 admits the cast/test operations without new cells or
allocation; JavaScript prototype reflection remains unfinished
([qualification](../evidence/task37-wasm-descriptor-casts.json)).
Initializer expressions retain one admitted representation: known values fold
inline, unresolved immutable globals remain symbolic. Global initializers,
active segment offsets and nullable func/extern reference initializers evaluate
against actual instance bindings through the same scalar operations; no imported
value is substituted during lowering
([qualification](../evidence/task37-wasm-initializer-expressions.json)).
`ref.func` is a symbolic dependency in that same expression model. Instantiation
roots its environment before installing defined globals in declaration order;
then table/segment effects and start execution proceed. Function references use
the original function cache/import identity and captured environment. No
placeholder globals or forwarding functions are constructed. Frontend reference
initializers share expression lowering
([qualification](../evidence/task37-wasm-function-initializers.json)).
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
([qualification](../evidence/task37-wasm-host-functions.json)).
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

## Wasm execution modes and link order

`coremark -no-jit` (default) runs Wasm on the shared interpreter; `-jit`
compiles each Wasm function with a compiled denotation through Cranelift
(`crates/quench-runtime/src/vm/compiled.rs`) and leaves the rest interpreted.
`run -jit` runs the testsuite in the compiled mode; both modes must pass the
whole suite. A module is lowered for the mode that runs it
(`Module::lower_shared_module_for`): the interpreter's residual inlines leaf
calls up to a large budget, since a call costs it a frame and a dispatch, while
the compiled mode keeps every call, since Cranelift calls cheaply and a copied
body only adds register pressure. The residual's meaning is the same in both.

Compiled code holds a Wasm vector in a 128-bit Cranelift variable (`Slot::Vector`) that
only SIMD instructions, moves and full-width vector accesses read; a function passing or
returning vectors, or reading one any other way, stays interpreted. Lane-wise integer
add/sub, shifts, bitwise logic, shuffles and `i64x2.extmul_low_i32x4_u` lower to Cranelift
vector instructions; every other operator calls its own kernel (`SimdKernel::apply_bits`),
the one the interpreter runs, so both modes agree bit for bit. `memory.copy` within the
function's memory checks both ranges, then moves the bytes as memmove does. JetStream 3
argon2-wasm `--jit`, whose compression rounds are SIMD: 0.52 → 55.9 (Node 26.12.0: 107.3;
Linux x86_64, one round, 2026-10-27); its argon2id hashes match Node's.

`QUENCH_WASM_JIT_PERF_MAP` (diagnostics) names each compiled Wasm function in
`/tmp/perf-PID.map` for `perf report` and writes the module's code to
`/tmp/quench-jit-PID-BASE.bin` for `objdump -b binary --adjust-vma=0xBASE`; the
`profile-lane-exits` feature reports, beside lane exits, the most dispatched interpreter
records by function and instruction index (`wasm-disasm` prints the same indices).

On Linux the harness binaries link position-dependent, and on x86-64 with
`ld.lld` available they link in the trained symbol order
`crates/quench-wasm-test/link/x86_64-linux.order`: executed functions share
pages instead of each faulting in its own fault-around window, which bounds
resident text. The order is layout only. Symbol names carry the profile's crate
hashes, so train on the profile you link (debug info and stripping change them;
`production` keeps its symbol table for this). Retrain after large changes from
cachegrind profiles of every Wasm kernel and CoreMark in both modes. Profiles
name executed functions (`--demangle=no`), so they stay valid across relinks;
cachegrind keeps no call stack, which the lane's sibling-call chains would
exhaust in callgrind:

```sh
cargo build --profile production -p quench-wasm-test --bin coremark --bin symbol-order
for module in KERNELS...; do for mode in no-jit jit; do
  COREMARK_CLOCK_STEPS_MS=1,1,2000 valgrind --tool=cachegrind --cache-sim=no \
    --demangle=no --cachegrind-out-file=train/$(basename $module)-$mode.out \
    target/production/coremark -$mode $module
done; done
target/production/symbol-order target/production/coremark \
  crates/quench-wasm-test/link/x86_64-linux.order \
  train/coremark-no-jit.out train/coremark-jit.out \
  train/KERNEL-no-jit.out... train/KERNEL-jit.out...
```

An order names symbols with the profile's crate hashes, so a change that moves them disables
it silently. `tools/check-link-order.sh BINARY ORDER` reports the share of the order's
symbols the binary still has and fails below 90%. Check `quench-node`'s production build
against `crates/quench-node/link/x86_64-linux.order` after manifest or dependency changes
(TOD-171).

Profiles go in priority order. Functions are laid out in first-appearance
order, so the first profile's run touches one contiguous region and later
profiles add only what earlier ones did not execute. With a compiled-mode
profile first, the interpreter's functions interleave with the compiler's and
fault-around makes the whole span resident (CoreMark `-no-jit`: 5.2 -> 6.1 MB).

Qualification builds link the C runtime statically, position-dependent:

```sh
RUSTFLAGS="-D warnings -C llvm-args=-align-all-functions=6 \
  -C target-feature=+crt-static -C relocation-model=static" \
  CARGO_TARGET_DIR=target/static cargo build --profile production \
  --target x86_64-unknown-linux-gnu -p quench-wasm-test --bin coremark --bin run
```

`RUSTFLAGS` replaces the workspace `build.rustflags`, so it repeats them; the
explicit `--target` keeps build scripts and proc macros dynamic. Only the C
runtime functions a run executes are then resident, laid out by the link
order instead of faulting in shared-library windows. Static builds name
symbols differently and use `link/x86_64-linux-static.order`, trained the same
way on the static binary. Their start code would register `.eh_frame` with
the unwinder, which reads the whole section at startup; `build.rs` resolves
the registration hooks to null instead (production aborts on panic, and
backtraces find frames through `PT_GNU_EH_FRAME`). CoreMark on the x86-64
container: dynamic -no-jit 5.2 MB / -jit 7.6 MB, static 3.5 MB / 6.3 MB.
A static position-independent build relocates its read-only pointers at
startup and measured 9.1 MB.

A static build cannot host native addons: an addon links the shared C runtime,
and loading it would run a second C runtime beside the static one, so
`process.dlopen` refuses with `ERR_DLOPEN_FAILED`. npm boards that run addon
scenarios (rollup, sharp, vitest's rolldown binding) use the dynamic build.

The static `quench-node` keeps its link relocations (`--emit-relocs`, never
loaded), and qualification rewrites its block layout with `llvm-bolt` (BOLT
20, Ubuntu `bolt-20`). A symbol order moves whole functions, but a run
executes about a third of the bytes of the functions it enters, and fault-around
keeps every 64 KB window it touches resident. `bolt-profile` (`quench-wasm-test`)
turns the order's callgrind profiles (taken with `--dump-instr=yes
--dump-line=no`) into a basic-sample profile, so BOLT moves each function's
executed blocks into hot text, in the trained function order, and the rest into
cold text. The interpreter's dispatch (`run_frame_general_until`, the lane
handlers) stays whole (`--dense`): a program reaches whichever blocks its code
needs. BOLT leaves code not generated from Rust in place (glibc's
single-thread atomics jump over a `lock` prefix, which BOLT splits), and
`symbol-order --foreign-first` packs that code at the start of the original
text. `bolt-profile` reads the symbols of the binary the profiles ran on and
writes counts per function offset, so the order's training profiles apply to
the relinked binary: convert them before relinking.

```sh
target/production/bolt-profile $PROFILED_BINARY bolt --order $ORDER \
  --dense 13dispatch_fast --dense run_frame_general_until run/*.out*
# relink with $ORDER, then:
llvm-bolt $BINARY -o $BINARY-bolt -data=bolt/profile.fdata \
  -reorder-blocks=ext-tsp -split-functions -split-all-cold \
  -reorder-functions=cdsort -skip-funcs-file=bolt/skip.funcs -use-gnu-stack \
  -jump-tables=move
```

`-reorder-functions=cdsort` orders hot functions by the profile's call graph instead of the
trained symbol order (`bolt/function.order`), which places them by first execution: callers
and callees a run alternates between share pages. A V8-v7 run had executed about 613 KB of
code lines spread over 1184 KB of pages; with the call-graph order its max RSS is 50-280 KB
lower on every fixture without and with `--jit` (bolted static x86_64, 2026-10-22: Richards
2340 -> 2064 KiB, Crypto 2956 -> 2768, NavierStokes 4180 -> 3996), Score within noise. The
linked order still lays out start-up and cold code.

The static `quench-node` order and its BOLT profile train on the same scripts without and
with `--jit` (start-up cachegrind and run-phase callgrind profiles of each), the `--jit`
profiles after the others: without them the JS tier's Cranelift compiler is laid out as
cold code, and a `--jit` run faulted in about 1.1 MB more of it (V8-v7 `--jit` max RSS,
bolted static x86_64, 2026-10-22: Crypto 4620 -> 3968 KiB, DeltaBlue 3908 -> 3368,
Richards 2724 -> 2468; no-JIT equal or lower).

The Wasm load path trains on `wasm-esm-corpus` (`quench-wasm-test`): the spec testsuite's
modules, written as files that its `run.mjs` imports through the Wasm ESM integration (those
importing `spectest` resolve it as a package), invoking each module's actions. Profile it
without `--jit` and with `--jit run.mjs returns`, which skips the actions expected to trap:
callgrind cannot follow the JIT's trap signals. Without these profiles the code of a Wasm
import lay scattered among npm and V8-v7 code: CoreMark imported as a module executed about
125 KB of code lines over 984 KB of pages during its run. Bolted with the corpus's profiles
alone, its max RSS without JIT fell 3784 -> 3296 KiB (static x86_64, 9 evicted runs, medians,
2026-10-25; Score equal). Training never uses CoreMark itself.

```sh
target/production/wasm-esm-corpus corpus crates/quench-wasm-test/testsuite/{address,block,br,\
br_if,br_table,bulk,call,call_indirect,conversions,data,elem,endianness,exports,f32,f64,fac,\
forward,func,global,i32,i64,if,imports,int_exprs,int_literals,labels,left-to-right,load,\
local_get,local_set,local_tee,loop,memory,memory_copy,memory_fill,memory_grow,memory_size,nop,\
ref_func,ref_is_null,ref_null,return,select,stack,start,store,switch,table,table_get,\
table_grow,table_set,table_size,traps,unwind}.wast
valgrind --tool=callgrind --dump-instr=yes --dump-line=no --demangle=no quench-node corpus/run.mjs
valgrind --tool=callgrind ... quench-node --jit corpus/run.mjs returns
```

Pass these profiles to `bolt-profile` after `--late`: an instruction only they execute counts
once, the lowest density BOLT orders, so `-reorder-functions=cdsort` lays the Wasm load path
behind the code the JavaScript profiles run instead of among it. At full weight they
interleaved with the JavaScript hot set and raised V8-v7 max RSS; after `--late` every row is
lower than with the JavaScript profiles alone. Bolted static x86_64, memory-first, 5 evicted
runs, medians, KiB, 2026-10-26; JavaScript profiles: the deegen curriculum, micros and an
empty script, without and with `--jit` (Scores within run-to-run spread):

| Row | JavaScript only | + Wasm, full weight | + Wasm, `--late` |
| -- | --: | --: | --: |
| CoreMark (Wasm ESM, no JIT) | 3356 | 3344 | 3128 |
| Crypto | 2756 | 2780 | 2660 |
| RegExp | 5176 | 5220 | 5108 |
| DeltaBlue | 2724 | 2696 | 2608 |
| Richards | 2172 | 2300 | 2064 |
| NavierStokes | 3736 | 3808 | 3632 |
| RayTrace | 2660 | 2644 | 2528 |

```sh
target/production/bolt-profile $PROFILED_BINARY bolt --order $ORDER \
  --dense 13dispatch_fast --dense run_frame_general_until run/*.out* --late wasm/*.out
```

`-jump-tables=move` lays the jump tables of hot code beside it, apart from the cold
ones: in place, the `match` tables of the dispatch and the compiler sit among
`.rodata`'s merged constants, and a run kept about 120 KB of them resident (V8-v7
max RSS, bolted static x86_64, 2026-10-22: 20-160 KB lower on every fixture).

`quench-node` registers its read-only file mappings with userfaultfd for
asynchronous write protection at start (`resident_pages::map_touched_pages_only`,
Linux 6.7 `UFFD_FEATURE_WP_ASYNC`): the kernel then skips fault-around in them,
so a fault maps only the touched page, not its cached 64 KB neighbourhood or
whole large folio. The mappings are never written, so the registration changes
nothing else; without the feature they keep fault-around. Resident code is then
what a run touches, and the block layout above decides how many pages that is.
Page-cache state matters far less once faults stop mapping neighbours (a
freshly written binary measured 0.3 MB above the same file evicted, against 8 MB
with fault-around), but comparisons still evict every binary first
(`posix_fadvise(POSIX_FADV_DONTNEED)`).

The static build also turns off glibc's per-thread cache and its heap's top padding
(`glibc.malloc.tcache_count=0:glibc.malloc.top_pad=0`): cached chunks stay unmerged, and
the heap grew past them by 70–280 KiB per V8-v7 fixture with the default seven per size
class, and by up to 400 KiB (RegExp) with two. glibc reads its tunables once, before
any code of the executable runs, so a run whose environment sets no `GLIBC_TUNABLES`
re-executes itself once from `.preinit_array` with the setting added, and the re-executed
run removes it and its marker before the guest or a child process can observe the
environment. An environment that sets `GLIBC_TUNABLES` leaves the allocator to it.
Valgrind does not follow the re-execution, so profiling and guard runs set
`GLIBC_TUNABLES=glibc.malloc.tcache_count=0:glibc.malloc.top_pad=0` themselves (`callgrind-guard.sh` does).

Measure peak RSS with a launcher whose own image is small (`vfork`, `exec`,
`wait4`): a forked interpreter's pre-`exec` image counts toward the child's
`ru_maxrss`.

The [no-JIT web framework benchmark](web-benchmarks.md) compares Quench, Node
and Bun using pinned upstream HTTP workloads and retains correctness and runtime
identity evidence for each framework/runtime combination.
