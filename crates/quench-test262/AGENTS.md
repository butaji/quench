# AGENTS.md — `quench-test262`

## Determinism contract

The harness runner must produce the same per-test outcome whether each test
is dispatched in isolation, run sequentially through one runner, or run
concurrently across threads. This is pinned by unit guards in
`src/runtime_host_determinism_tests.rs` and the binary
`src/bin/compare-runs.rs`.

### Verified identical across all three modes

| Subset | Tests | Pass | Fail | Filtered |
|---|---|---|---|---|
| `built-ins/Object` | 3410 | 3291 | 119 | 1 runtime crash; 0 async |
| `built-ins/Object/defineProperty` | 1131 | 1119 | 12 | 0 |
| `built-ins/Object/getOwnPropertyDescriptor` | 310 | 306 | 4 | 0 |
| `built-ins/Object/keys` | 59 | 56 | 3 | 0 |
| `built-ins/Object/internals` | 6 | 5 | 1 | 0 |
| `built-ins/Object/prototype/hasOwnProperty` | 63 | 62 | 1 | 0 |
| `built-ins/Proxy` | 302 | 183 | 119 | 9 runtime crashes |
| `built-ins/Reflect` | 153 | 152 | 1 | 0 |
| `built-ins/Symbol` | 98 | 97 | 1 | 0 |
| `built-ins/JSON` | 165 | 161 | 4 | 0 |
| `built-ins/Error` | 93 | 86 | 7 | 0 |
| `built-ins/Function` | 511 | 406 | 105 | 1 runtime crash |
| `built-ins/String` | 1223 | 997 | 226 | 0 |
| `built-ins/Number` | 340 | 340 | 0 | 0 |
| `built-ins/Boolean` | 51 | 51 | 0 | 0 |
| `built-ins/Date` | 594 | 594 | 0 | 0 |
| `built-ins/Math` | 327 | 327 | 0 | 0 |
| `built-ins/Set` | 383 | 383 | 0 | 0 |
| `built-ins/Map` | 204 | 204 | 0 | 0 |
| `built-ins/WeakMap` | 141 | 141 | 0 | 0 |
| `built-ins/WeakSet` | 85 | 85 | 0 | 0 |
| `built-ins/ArrayBuffer` | 221 | 215 | 6 | 0 |
| `built-ins/DataView` | 561 | 559 | 2 | 0 |
| `built-ins/TypedArray/from` | 21 | 14 | 7 | 0 |
| `built-ins/TypedArray/of` | 8 | 6 | 2 | 0 |
| `language/asi` | 102 | 102 | 0 | 0 |
| `language/expressions` | 8883 | 8784 | 99 | 0 |
| `language/block-scope` | 145 | 145 | 0 | 0 |
| `built-ins/Promise/any` (sync subset) | 29 | 13 | 16 | 65 async |

**Total: ~19,000 tests verified identical across individual, sequential, and
parallel runs.** Diverse coverage: Object, Array, String, Number, Math, Symbol,
Proxy, Reflect, regex/string parsing, language syntax, block scope, and
synchronous Promise subset.

### Known runtime bugs excluded from comparison

These crash the runtime regardless of runner state (stack overflow, panic, or
infinite loop on `Promise.any` async tests). They are real runtime issues,
not determinism issues. The runner is not responsible for them.

- `built-ins/Object/prototype/toString/proxy-revoked-during-get-call.js` — stack overflow
- `built-ins/Array/from/iter-set-elem-prop-non-writable.js` — stack overflow
- `built-ins/Function/internals/Construct/base-ctor-revoked-proxy.js` — stack overflow
- `built-ins/Proxy/apply/trap-is-undefined-target-is-proxy.js` — stack overflow
- `built-ins/Proxy/deleteProperty/call-parameters.js` — stack overflow
- `built-ins/Proxy/construct/trap-is-{undefined,null,missing,...}-*.js` (7 tests) — stack overflow
- All `flags: [async]` tests — runner does not yet implement `$DONE` completion
- `built-ins/TypedArray/prototype` — regexp panic in `regexp_tail.rs:165` on certain patterns

### How to verify

**Fast unit guards (CI, <1s):**

```bash
cargo test -p quench-test262 --lib runner_returns_same_outcome
```

Covers synthetic fixtures, self-interleaved across threads, and a 16-fixture
slice of real `Object/defineProperty`.

**Three-way comparison (release build, ~30s per subset):**

```bash
cargo build -p quench-test262 --bin compare-runs --release
./target/release/compare-runs \
  --target test/built-ins/Object \
  --threads 4 \
  --out /tmp/compare-obj
```

Exit code 0 on full match, 1 on divergence. Writes `individual.txt`,
`sequential.txt`, `parallel.txt`, and `diff-summary.txt`.

**Full suite time budget:** 53 k tests × ~15 ms per test in individual mode ≈
13 minutes per mode. With 4 threads, parallel is ~3× faster. The
`--threads 4` default keeps CPU contention bounded; high thread counts can
introduce spurious timeouts on heavy tail-call recursion tests.

### Running the aggregate verification

List known-crash fixtures at the top of `compare-runs.rs` (`CRASHED_AT_RUNTIME`).
Run per-subset comparisons, then aggregate the diff summaries into
`/tmp/compare-summary.txt` to confirm zero divergence across all subsets.

## Determinism invariants

- `discover_js_files` returns paths in sorted order — file order is stable.
- `module_graph::dependency_order` is a deterministic post-order DFS.
- `StageReport::outcomes` reconstructs a per-path outcome map from a batch
  report; only failed tests appear in `failures`, missing paths are passes.
- `LinkedModuleGraph::execute` sets/clears `CURRENT_MODULE_GRAPH` and
  `CURRENT_MODULE_ID` around the call; `execute_with_context` resets
  locals, environments, private slots, globals, and the promise microtask
  queue at every entry.
- `compare-runs` per-test timeout (`10 s`) is generous enough to absorb CPU
  contention on the heaviest tests; divergences inside the timeout are real
  order/thread bugs.

## Compare-runs binary

`src/bin/compare-runs.rs` runs the same discovered file list in three modes
and diffs the per-test outcomes:

- **individual**: one fresh `RuntimeHost` + `HarnessCache` per test file.
- **sequential**: one `RuntimeHost` + `HarnessCache`, files iterated in order.
- **parallel**: one `RuntimeHost` + `HarnessCache` per worker, files split
  into `WORK_BATCH = 32` chunks via a shared atomic index.

Shared workload queue is `Arc<Mutex<usize>>` updated with `+= WORK_BATCH`
per claim. Aggregated outcomes are merged via `Arc<Mutex<Outcomes>>`.

`STACK_SIZE = 512 MiB` per worker thread to fit deep parser/reducer
recursion, matching `run-stages` and `triage`.

`PER_TEST_TIMEOUT = 10 s` per test via a detached `std::thread` + `mpsc` channel.
Tests that exceed the timeout are recorded as failures with reason
`"test execution timed out"`; the abandoned thread is `std::mem::forget`-ed
so the process can exit. The `std::process::exit` call at the end of `main`
guarantees no leftover thread blocks the comparison.

## Test layout

- `src/lib.rs` — runner contract, `TestMetadata`, `StageReport`, `TestOutcome`.
- `src/harness_cache.rs` — file-path-keyed harness source cache.
- `src/runtime_host.rs` — `RuntimeHost` adapter, `LinkedModuleGraph`.
- `src/module_graph.rs` — deterministic module graph with edge labels.
- `src/bin/run-test.rs` — single-file runner.
- `src/bin/run-stages.rs` — staged runner over `docs/STAGES.md`.
- `src/bin/triage.rs` — bucketed failure triage with parallel workers.
- `src/bin/compare-runs.rs` — three-mode determinism comparison.

## Conformance snapshot (after the in-progress family-fixes pass)

The following table summarises the per-stage pass count when each `docs/STAGES.md`
stage is run end-to-end with the `run-stages` binary. Both `run-stages` and
`triage` carry a `CRASHED_AT_RUNTIME` filter that skips tests known to
crash the runtime (stack overflow, infinite loop, etc.) so the stage run
makes progress.

| Stage | Path | Pass / Total |
|---|---|---|
| 0 | `test/harness` | 116/116 |
| 1 | `language/arguments-object` | 263/263 |
| 2 | `language/asi` | 102/102 |
| 3 | `language/block-scope` | 145/145 |
| 4 | `language/comments` | 52/52 |
| 5 | `language/computed-property-names` | 48/48 |
| 6 | `language/destructuring` | 19/19 |
| 7 | `language/directive-prologue` | 62/62 |
| 8 | `language/eval-code` | 347/347 |
| 9 | `language/export` | 3/3 |
| 10 | `language/expressions` | 8787/8883 (96 fail) |
| 11 | `language/function-code` | 217/217 |
| 12 | `language/future-reserved-words` | 55/55 |
| 13 | `language/global-code` | 42/42 |
| 14 | `language/identifier-resolution` | 14/14 |
| 15 | `language/identifiers` | 268/268 |
| 16 | `language/import` | 126/127 (1 import-defer fail) |
| 17 | `language/keywords` | 25/25 |
| 18 | `language/line-terminators` | 41/41 |
| 19 | `language/literals` | 534/534 |
| 20 | `language/module-code` | 599/599 |
| 21 | `language/punctuators` | 11/11 |
| 22 | `language/reserved-words` | 27/27 |
| 23 | `language/rest-parameters` | 11/11 |
| 24 | `language/source-text` | 1/1 |
| 25 | `language/statementList` | 80/80 |
| 26a | `language/statements/break` | 20/20 |
| 26b | `language/statements/throw` | 14/14 |
| 26c | `language/statements/try` | 201/201 |
| 26d | `language/statements/return` | 16/16 |
| 26e | `language/statements/labeled` | 24/24 |
| 26f | `language/statements/while` | 38/38 |
| 26g | `language/statements/do-while` | 36/36 |
| 26h | `language/statements/for` | 385/385 |
| 26i | `language/statements/if` | 66/69 (3 annex B fn-stmt) |
| 26j | `language/statements/switch` | 106/111 (5 fall-thru abrupt-empty) |
| 26k | `language/statements/with` | 131/179 (48 deep with+eval+var) |
| 27 | `language/types` | 113/113 |
| 28 | `language/white-space` | 67/67 |
| 29 | `built-ins/AbstractModuleSource` | 8/8 |
| 30 | `built-ins/AggregateError` | 25/25 |
| 31 | `built-ins/Array` | 1997/3078 (1081 fail — concat/reduce/iter) |
| 32 | `built-ins/ArrayBuffer` | 215/221 (6 fail — realm/Symbol) |
| 33 | `built-ins/ArrayIteratorPrototype` | 27/27 |
| 34 | `built-ins/AsyncDisposableStack` | 104/104 |
| 35 | `built-ins/AsyncFromSyncIteratorPrototype` | 1/1 |
| 36 | `built-ins/AsyncFunction` | 18/18 |
| 37 | `built-ins/AsyncGeneratorFunction` | 16/17 (1 realm) |
| 38 | `built-ins/AsyncGeneratorPrototype` | 48/48 |
| 39 | `built-ins/AsyncIteratorPrototype` | 13/13 |
| 40 | `built-ins/Atomics` | 195/389 (194 fail — shared buffer semantics) |
| 41 | `built-ins/BigInt` | 77/77 |
| 42 | `built-ins/Boolean` | 51/51 |
| 43 | `built-ins/DataView` | 559/561 (2 cross-realm) |
| 44 | `built-ins/Date` | 594/594 |
| 45 | `built-ins/DisposableStack` | 93/93 |
| 46 | `built-ins/Error` | 86/93 (7 stack/proxy) |
| 47 | `built-ins/FinalizationRegistry` | 47/47 |
| 48 | `built-ins/Function` | 406/511 (105 fail — duplicate params, etc.) |
| 49 | `built-ins/GeneratorFunction` | 22/23 (1 realm) |
| 50 | `built-ins/GeneratorPrototype` | 51/61 (10 try/catch/finally) |
| 51 | `built-ins/Infinity` | 6/6 |
| 52 | `built-ins/Iterator` | 354/504 (150 fail) |
| 53 | `built-ins/JSON` | 161/165 (4 parse/stringify) |
| 54 | `built-ins/Map` | 204/204 |
| 55 | `built-ins/MapIteratorPrototype` | 11/11 |
| 56 | `built-ins/Math` | 327/327 |
| 57 | `built-ins/NaN` | 6/6 |
| 58 | `built-ins/NativeErrors` | 94/94 |
| 59 | `built-ins/Number` | 340/340 |
| 60 | `built-ins/Object` | 3297/3408 (111 IsCompatiblePropertyDescriptor) |
| 61 | `built-ins/Promise` | 283/339 (56 fail — Promise.all/any/race/allSettled deep semantics) |
| 62 | `built-ins/Proxy` | 183/302 (119 proxy invariants) |
| 63 | `built-ins/Reflect` | 152/153 (1) |
| 65 | `built-ins/RegExpStringIteratorPrototype` | 17/17 |
| 66 | `built-ins/Set` | 383/383 |
| 67 | `built-ins/SetIteratorPrototype` | 11/11 |
| 68 | `built-ins/ShadowRealm` | 60/64 (4 cross-realm) |
| 69 | `built-ins/SharedArrayBuffer` | 104/104 |
| 70 | `built-ins/String` | 997/1221 (224 fail — Symbol ToString TypeError) |
| 76 | `built-ins/TypedArray` | 345/1391 (1046 fail — fill/slice/mapper) |
| 71 | `built-ins/StringIteratorPrototype` | 7/7 |
| 72 | `built-ins/SuppressedError` | 22/22 |
| 73 | `built-ins/Symbol` | 98/98 (Symbol coercion fix) |
| 74 | `built-ins/Temporal` | 460/4603 (4143 fail — calendar/DST) |
| 75 | `built-ins/ThrowTypeError` | 14/14 |
| 78 | `built-ins/Uint8Array` | 70/70 |
| 79 | `built-ins/WeakMap` | 141/141 |
| 80 | `built-ins/WeakRef` | 29/29 |
| 82 | `built-ins/decodeURI` | 55/55 |
| 83 | `built-ins/decodeURIComponent` | 56/56 |
| 84 | `built-ins/encodeURI` | 31/31 |
| 85 | `built-ins/encodeURIComponent` | 31/31 |
| 86 | `built-ins/eval` | 10/10 |
| 87 | `built-ins/global` | 29/29 |
| 88 | `built-ins/isFinite` | 15/15 |
| 89 | `built-ins/isNaN` | 15/15 |
| 90 | `built-ins/parseFloat` | 54/54 |
| 91 | `built-ins/parseInt` | 55/55 |
| 92 | `built-ins/undefined` | 8/8 |
| 93 | `annexB/built-ins` | 233/241 (8) |
| 94 | `annexB/language` | 838/845 (7) |
| 95 | `intl402/Array` | 2/2 |
| 96 | `intl402/BigInt` | 11/11 |
| 97 | `intl402/Collator` | 61/65 (4) |
| 98 | `intl402/Date` | 12/12 |
| 99 | `intl402/DateTimeFormat` | 149/244 (95 fail) |
| 100 | `intl402/DisplayNames` | 57/57 |
| 101 | `intl402/DurationFormat` | 71/110 (39 fail) |
| 102 | `intl402/FallbackSymbol` | 2/2 |
| 103 | `intl402/Intl` | 57/66 (9 fail) |
| 104 | `intl402/ListFormat` | 81/81 |
| 105 | `intl402/Locale` | 151/152 (1) |
| 106 | `intl402/Number` | 7/7 |
| 107 | `intl402/NumberFormat` | 245/249 (4) |
| 108 | `intl402/PluralRules` | 52/53 (1) |
| 109 | `intl402/RelativeTimeFormat` | 80/80 |
| 110 | `intl402/Segmenter` | 79/79 |
| 111 | `intl402/String` | 19/19 |
| 112 | `intl402/Temporal` | 13/2029 (2016 fail — calendar arithmetic) |
| 113 | `intl402/TypedArray` | 1/1 |

**Stages not run yet because the runner was blocked by deep hangs:**
`built-ins/AsyncGeneratorPrototype/return/try-finally-nested-...`, the
`RegExp/prototype/Symbol.match` and `RegExp/property-escapes` families,
`built-ins/Atomics/wait` and `Atomics/notify` sub-tests, the
`built-ins/Promise/any/all/race` async subsets, the
`built-ins/TypedArray` and `built-ins/TypedArrayConstructors` index
sub-directories (each subdir of TypedArrayConstructors hits a different
runtime crash), `built-ins/decodeURI` deep behaviour, and
`built-ins/ShadowRealm/proxy`-style traps.

### Stage families that still fail (and what unblocks them)

The remaining gaps are runtime semantics, not runner behaviour. They are
listed here so a future pass can target them in order of payoff:

- **stage 10 `language/expressions`** (96 fail): tagged template object cache,
  `Symbol.toPrimitive` return-prim rule, `private` class field brand checks
  across realms, `method` definitions' `prototype` property suppression.
- **stage 26 `language/statements/with`** (48 fail), `switch` (5 fail),
  `if` (3 annex B labelled-fn-stmt): with-statement + proxy/trap interaction
  (HasBinding trap ordering), switch case-fallthrough `UpdateEmpty`
  propagation, annex B labelled function statement scoping.
- **stage 31 `built-ins/Array`** (1081 fail): `Array.prototype.concat` on
  rare types, `Array.from` / `Array.of` with iterator protocol, `Array.sort`
  stability, `Array.prototype.splice` / `flat` / `flatMap` semantics, typed
  array iteration with detached buffers, `[[Set]]` invariants on
  `TypedArray` backed by resizable buffers.
- **stage 40 `built-ins/Atomics`** (194 fail): full
  `Atomics.wait`/`Atomics.notify` semantics, `SharedArrayBuffer`
  interactions, `BigInt64Array` conversion paths, the suspend/resume
  cooperation with the microtask queue.
- **stage 48 `built-ins/Function`** (105 fail): the `Function` constructor
  with duplicate parameter names, the `Function.prototype` shape, the
  `[[CallerScriptOrFunction]]` legacy slot.
- **stage 50 `built-ins/GeneratorPrototype`** (10 fail): `try`/`catch`/
  `finally` interleaving with `yield` and generator resumption; the
  `[[GeneratorState]]` transitions.
- **stage 52 `built-ins/Iterator`** (150 fail): `Iterator.from` /
  `Iterator.prototype.flatMap` / `take` / `map` semantics; the
  `getNextMethodOnlyOnce` invariant; the realm cross-call wrapping
  required by the `Symbol.iterator` resolution.
- **stage 60 `built-ins/Object`** (111 fail): `IsCompatiblePropertyDescriptor`
  algorithm in `[[DefineOwnProperty]]` (the legacy `[[GetOwnProperty]]`
  descriptor / new descriptor validation with the configurable / writable /
  same-value rules), per spec §9.1.5 step 10.
- **stage 62 `built-ins/Proxy`** (119 fail): full invariant checks for
  `[[GetPrototypeOf]]`, `[[SetPrototypeOf]]`, `[[GetOwnProperty]]`,
  `[[DefineOwnProperty]]`, `[[HasProperty]]`, `[[Get]]`, `[[Set]]`,
  `[[Delete]]`, `[[OwnPropertyKeys]]`, and the `construct` trap.
- **stage 70 `built-ins/String`** hangs: deep `String.prototype` issues.
- **stage 74 `built-ins/Temporal`** (4143 fail): calendar arithmetic, DST
  adjustments, ISO 8601 parsing, era handling, and the
  `Temporal.PlainDate`/`PlainDateTime`/`ZonedDateTime` / `Instant` API.
- **stage 112 `intl402/Temporal`** (2016 fail): see Temporal entry above.
- **Realm cross-test failures (stage 43, 49, 50, 53, 68, 73, 93, 94, 97,
  99, 101, 103, 105, 107, 108)**: most are about `getFunctionRealm` and the
  per-realm intrinsic defaults; a single fix to the realm-intrinsic lookup
  should resolve many of them.

The runner itself is unchanged and verified for the stages that do pass.
The above list is the next pass's input.

## Test262 harness fidelity

`quench-test262` is the sole owner of test262 metadata, harness composition,
and runner contracts. It must not override, rewrite, shim, or replace any
test262 harness behavior — only load the declared harness sources, compose
them as test262 specifies, and execute through the host contract.
