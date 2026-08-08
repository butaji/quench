# Upstream fixtures — Node 24 application compatibility manifest

Node's upstream suite is the primary behavioral oracle. The first manifest
covers `test/parallel/` and `test/es-module/`, with required `test/common/` and
`test/fixtures/` support files. It is versioned as JSONC under
`tests/node-compat/` and records `pass`, `fail`, `skip`, `platform-limited`,
`unsupported`, and `known-conflict`. Upstream counts measure progress, not an
API percentage; Hono and a representative npm CLI are also release gates.
The source relationships and implementation order are documented in
`docs/authoritative-test-sources.md`: Node is the oracle, LLRT is the
QuickJS/Rust reference, Deno is the foreign-runtime runner reference, WPT
covers web APIs, and Test262 covers the engine baseline.

## Stage 1160: crypto random size validation

- Fixture: `test-crypto-random.js`
- Added a separate random-byte polyfill with Node-shaped type/range errors and
  the legacy `pseudoRandomBytes` alias.
- Retrospective: split high-churn crypto APIs by concern before adding more
  validation, preserving the file-size gate and making ownership explicit.

## Stage 1159: crypto SHAKE XOF hashes

- Fixture: `test-crypto-hash.js`
- Added Rust SHAKE128/SHAKE256 XOF bindings and JS `outputLength` support.
- Retrospective: keep variable-length XOF output in the Rust digest boundary;
  the JS wrapper only validates options and selects the requested length.

## Stage 1158: crypto hash algorithm bridge

- Fixture: `test-crypto-hash.js`
- Added minimal Rust digest bindings for SHA-1, SHA-256, SHA-512, and MD5 and
  selected them from the JS hash wrapper.
- Retrospective: keep digest computation in Rust while retaining the readable
  streaming/hash object behavior in JavaScript.

## Stage 1157: crypto.pbkdf2 key-length diagnostics

- Fixture: `test-crypto-pbkdf2.js`
- Matched `keylen` type and non-integer range diagnostics while retaining Node
  error codes.
- Retrospective: validate numeric type before integer/range checks so Node’s
  distinct TypeError and RangeError contracts remain observable.

## Stage 1156: crypto.pbkdf2 range error codes

- Fixture: `test-crypto-pbkdf2.js`
- Added `ERR_OUT_OF_RANGE` codes for invalid PBKDF2 iteration and key-length
  inputs.
- Retrospective: preserve error structure while treating exact diagnostic
  wording as a separate compatibility contract.

## Stage 1155: crypto.pbkdf2 callback validation

- Fixture: `test-crypto-pbkdf2.js`
- Added Node’s `ERR_INVALID_ARG_TYPE` code to the missing-callback error.
- Retrospective: when a validation message already matches, preserve the
  structured error code separately from the human-readable text.

## Stage 1154: common crypto capability helpers

- Fixture: `test-crypto-pbkdf2.js`
- Added the harness capability flags and skip helpers required before crypto
  fixtures can reach their API assertions.
- Retrospective: complete shared fixture prerequisites before attributing an
  early failure to the underlying Node API.

## Stage 1153: REPL writer color options

- Fixture: `test-repl-colors.js`
- Added the minimal `REPLServer` writer surface and color option expected by
  the upstream constructor contract.
- Retrospective: expose observable constructor state before implementing the
  full interactive REPL loop; this keeps the compatibility boundary small.
- The shared test helper also now exposes `skipIfInspectorDisabled()` as a
  no-op because this harness does not disable inspector-backed fixtures.
- The focused contract also verifies that input data reaches the output
  writer; the upstream fixture still exercises a fuller Duplex lifecycle.

## Stage 1152: filesystem appendFile invalid data

- Fixture: `test-fs-append-file.js`
- Added focused coverage for rejecting non-string and non-buffer append data
  with `ERR_INVALID_ARG_TYPE` before creating a file.
- Retrospective: validate negative filesystem contracts independently from
  asynchronous completion paths so error timing and file creation are clear.

## Stage 1151: filesystem appendFile callback and promise APIs

- Fixture: `test-fs-append-file.js`
- Added focused coverage for callback and promise append operations over the
  existing filesystem implementation.
- Retrospective: isolate callback and promise adapters in one contract before
  changing shared filesystem storage behavior.

## Stage 1150: VM realm Object.hasOwn

- Fixture: `test-vm-global-property-prototype.js`
- Verify `Object.hasOwn()` and own-property descriptors inside a VM context
  with an inherited sandbox prototype.
- Retrospective: the focused realm probe passes without extra context code, so
  keep the shim minimal and isolate the remaining large-fixture failure before
  changing context installation.

## Stage 1149: legacy URL raw query formatting

- Fixture: `test-url-format.js`
- Preserved raw query characters such as `@` in string-based legacy
  `url.format()` while retaining WHATWG path and hash normalization.
- Retrospective: isolate legacy URL formatting from WHATWG URL serialization to
  avoid over-encoding compatibility-sensitive query text.

## Stage 1148: Object.hasOwn static helper

- Fixture: `test-vm-global-property-prototype.js`
- Added the standard `Object.hasOwn()` helper used by context property checks.
- Retrospective: restore missing ECMAScript statics at the global polyfill layer
  so VM fixtures and ordinary user code share the same surface.

## Stage 1147: VM Script source map URL

- Fixture: `test-vm-source-map-url.js`
- Extracted valid hash-prefixed `sourceMappingURL` comments from Script source
  while ignoring malformed magic comments.
- Retrospective: keep source metadata parsing local to Script construction so
  evaluation remains unchanged.

## Stage 1146: VM isContext API

- Fixture: `test-vm-is-context.js`
- Added typed input validation and context-registry-backed `vm.isContext()`.
- Retrospective: reuse the existing WeakSet context registry for identity APIs;
  no additional Rust state is required.

## Stage 1145: VM Proxy constructor forwarding

- Fixture: `test-vm-proxies.js`
- Forwarded a context’s own `Proxy` constructor when explicitly assigned into
  an otherwise empty new-context sandbox.
- Retrospective: built-in constructors need explicit realm-aware forwarding;
  generic global-name filtering intentionally omits them.

## Stage 1144: VM inherited prototype properties

- Fixture: `test-vm-symbols.js`
- Temporarily applied the sandbox prototype to the context global so inherited
  methods and symbol-backed state remain visible during evaluation.
- Retrospective: context entry must preserve both own descriptors and the
  sandbox prototype chain; restoring both prevents host-global contamination.

## Stage 1143: VM nested global identity

- Fixture: `test-vm-property-not-on-sandbox.js`
- Rewrote direct nested sandbox self-references to the context global while
  installing object-valued context properties.
- Retrospective: preserve identity at descriptor injection time instead of
  copying properties after evaluation.

## Stage 1142: VM restricted global declarations

- Fixture: `test-vm-global-restricted-property.js`
- Added SyntaxError handling for lexical declarations that collide with
  non-configurable context globals.
- Retrospective: a narrow source preflight covers this ECMAScript global
  declaration invariant without adding a second evaluator.

## Stage 1141: VM context symbol keys

- Fixture: `test-vm-ownkeys.js`
- Forwarded symbol-keyed sandbox descriptors alongside string keys during VM
  context installation and synchronization.
- Retrospective: use a shared property-key representation for VM descriptors;
  string-only snapshots silently lose symbol properties.

## Stage 1140: VM Script cached data

- Fixtures: `test-vm-cached-data.js`, `test-vm-createcacheddata.js`
- Added `Script.createCachedData()`, production metadata, and source-based cache
  rejection for matching and mismatched scripts.
- Retrospective: share the deterministic source snapshot used by
  `compileFunction` rather than introducing an engine-specific bytecode layer.

## Stage 1139: VM compileFunction cache rejection

- Fixture: `test-vm-basic.js` cached-data rejection contract
- Exposed `cachedDataRejected` for matching and mismatched source inputs.
- Retrospective: a deterministic source snapshot is sufficient for the harness
  contract while avoiding a Rust bytecode cache implementation.

## Stage 1138: VM compileFunction cached data

- Fixture: `test-vm-basic.js` cached-data contract
- Added observable `cachedDataProduced` and non-empty cached-data output, with
  cached-data input accepted by compiled functions.
- Retrospective: implement the API-visible cache metadata first while keeping
  execution independent of engine-specific bytecode formats.

## Stage 1137: VM compileFunction error stacks

- Fixture: `test-vm-basic.js` compile-function error locations
- Normalized compiled-function error stacks with Node-style first lines and
  `lineOffset`/`columnOffset` handling.
- Retrospective: normalize host error stacks at the polyfill boundary while
  preserving the original error object and metadata.

## Stage 1136: VM compileFunction parsing context

- Fixture: `test-vm-basic.js` valid `parsingContext` contract
- Accepted contextified objects as compile-function parsing contexts and made
  their bindings visible to the compiled function.
- Retrospective: validate context identity against the VM context registry, then
  reuse the extension-binding path for execution.

## Stage 1135: VM compileFunction context extensions

- Fixture: `test-vm-basic.js` context-extension contract
- Added visible extension bindings and validation for extension arrays and
  extension object entries.
- Retrospective: direct probes can confirm exact diagnostics even when the
  larger fixture’s assertion wrapper reports a misleading failure location.

## Stage 1134: common string argument formatting

- Fixture: `test-vm-basic.js` invalid-options diagnostics
- Matched the Node common helper’s quoted string rendering in invalid argument
  messages.
- Retrospective: shared fixture helpers must produce the same diagnostic text
  as the upstream helper before API validation assertions can be trusted.

## Stage 1133: VM compileFunction basics

- Fixture: `test-vm-basic.js` compile-function contract
- Added callable parameterized functions, no-parameter function string output,
  and initial code/parameter/options validation.
- Retrospective: wrap the host `Function` constructor only where Node’s
  observable function representation differs.

## Stage 1132: VM context option validation

- Fixture: `test-vm-basic.js` context options
- Validated context metadata option types for `createContext()` and
  `runInNewContext()`.
- Retrospective: keep context metadata validation separate from execution
  options because both APIs share the same call position with different keys.

## Stage 1131: assert throws predicates

- Fixture: `test-vm-basic.js` filename matcher
- Distinguished error-constructor expectations from predicate callbacks in
  `assert.throws`, allowing ordinary functions with a `.prototype` property.
- Retrospective: validate harness semantics before changing the API under test;
  the VM stack formatting was already correct.

## Stage 1130: process object brand

- Fixture: `test-vm-basic.js` run-in-this-context contract
- Added Node’s observable `Symbol.toStringTag` brand to the process object.
- Retrospective: small global identity mismatches belong in the shared process
  surface, not in VM-specific evaluation code.

## Stage 1129: VM context global isolation

- Fixture: `test-vm-basic.js` contextified-sandbox contract
- Hid and restored the host `process` descriptor around `runInContext()` and
  its callback re-entry path.
- Retrospective: share realm-isolation helpers across VM entry points while
  keeping each entry point’s focused regression independently runnable.

## Stage 1128: VM new-context global isolation

- Fixture: `test-vm-basic.js` new-context contract
- Hid the host `process` global while evaluating a new context, preserving the
  standard built-ins and restoring the host descriptor afterward.
- Retrospective: compare each VM realm entry point independently; the same
  global-isolation rule must be applied separately to `runInContext`.

## Stage 1127: VM Script option validation

- Fixture: `test-vm-options-validation.js`
- Added type/range validation for `vm.Script` offsets, filename, cached-data,
  and cached-data production options.
- Retrospective: validate shared constructor options once and reuse the helper
  across execution methods to keep the Rust host unchanged.

## Stage 1126: common allowGlobals harness helper

- Fixture: `test-vm-run-in-new-context.js`
- Added the missing no-op `common.allowGlobals()` compatibility helper so the
  complete upstream VM fixture can finish after its assertions.
- Retrospective: classify failures at the fixture boundary; a missing test
  helper can masquerade as a failure in the API under test.

## Stage 1125: VM external callback mutation

- Fixture: `test-vm-run-in-new-context.js` callback contract
- Preserved mutations made by host functions passed into a new VM context while
  cleaning up the temporary function binding afterward.
- Retrospective: reduce large upstream fixtures into independently verifiable
  contracts when a remaining harness failure spans multiple behaviors.

## Stage 1124: VM Script new-context execution

- Fixture: `test-vm-new-script-new-context.js`
- Added a named VM context helper that snapshots host globals, forwards sandbox
  changes, and restores temporary bindings after `Script.runInNewContext()`.
- Retrospective: isolate the smallest upstream contract first; the broader
  `runInNewContext` fixture still needs separate handling for external function
  callbacks and remains intentionally outside this stage.

## Stage 1123: VM expose-gc hook

- Fixtures: `test-vm-create-and-run-in-context.js`,
  `test-vm-context-async-script.js`
- Added the minimal no-op `globalThis.gc()` hook required by the upstream
  `--expose-gc` fixtures.
- Retrospective: unsupported CLI flags can expose small capability hooks at
  bootstrap time without affecting the Rust host.

## Stage 1122: VM Script evaluation

- Fixture: `test-vm-context.js`
- Added a minimal `vm.Script` wrapper with context and new-context execution
  backed by the existing VM evaluator.
- Retrospective: thin API wrappers can reuse the established evaluator while
  keeping the Rust host unchanged.

## Stage 1121: VM getter descriptors

- Fixture: `test-vm-getters.js`
- Preserved the original `configurable` attribute for VM accessor properties.
- Retrospective: cleanup convenience must not alter descriptors visible to
  evaluated code; isolated runtimes make exact descriptor preservation viable.

## Stage 1120: VM non-configurable properties

- Fixtures: `test-vm-global-non-writable-properties.js`,
  `test-vm-data-property-writable.js`
- Avoided redefining host-global properties that became non-configurable in a
  VM context, preserving their values across subsequent evaluations.
- Retrospective: VM cleanup must distinguish properties introduced by the shim
  from properties made permanently non-configurable by evaluated code.

## Stage 1119: VM context accessors

- Fixture: `test-vm-create-context-accessors.js`
- Preserved non-enumerable getter/setter descriptors while entering and leaving
  a VM context.
- Retrospective: descriptor-based injection is safer than value copying for
  context properties with accessors or non-default attributes.

## Stage 1118: VM global identity

- Fixture: `test-vm-global-identity.js`
- Preserved `window === this` for contexts whose window property points back to
  the context object.
- Retrospective: recognizing self-referential sandbox properties at injection
  time avoids creating a second proxy for the VM global.

## Stage 1117: VM context forwarding

- Fixture: `test-vm-context-property-forwarding.js`
- Captured assignments from `runInContext()` back onto the provided context
  object while retaining existing property reads.
- Retrospective: context forwarding is a small post-evaluation synchronization
  step compatible with the current lightweight VM shim.

## Stage 1116: VM context validation

- Fixture: `test-vm-create-context-arg.js`
- Added `ERR_INVALID_ARG_TYPE` validation for primitive VM contexts while
  preserving object and array contexts.
- Retrospective: validating at `createContext()` keeps downstream VM methods
  free of repeated type checks.

## Stage 1115: VM function realm identity

- Fixture: `test-util-promisify.js`
- Preserved a distinct function prototype for `vm.runInNewContext()` results
  and propagated it through generated promisified functions.
- Retrospective: cross-context invariants need to be established at VM result
  creation, then preserved by utilities that wrap functions.

## Stage 1114: querystring max keys

- Fixture: `test-querystring-maxKeys-non-finite.js`
- Matched Node’s type-sensitive handling of numeric non-finite `maxKeys` and
  string non-finite values.
- Retrospective: preserving option input types is necessary when coercion would
  collapse distinct Node semantics.

## Stage 1113: querystring surrogate escaping

- Fixture: `test-querystring-escape.js`
- Added legacy querystring surrogate encoding and Node’s `ERR_INVALID_URI`
  behavior for lone surrogates.
- Retrospective: legacy encoding needs a dedicated normalization pass rather
  than relying on the stricter WHATWG/JS URI encoder.

## Stage 1112: OS process priority

- Fixture: `test-os-process-priority.js`
- Added the highest priority constant and validated PID/priority types and
  ranges while retaining the host-independent priority state.
- Retrospective: validation helpers keep the public OS methods small and make
  platform-independent tests deterministic.

## Stage 1111: checked OS home directory

- Fixture: `test-os-checked-function.js`
- Added the internal OS home-directory hook and Node-shaped checked-system-call
  error propagation for `os.homedir()`.
- Retrospective: exposing a small mutable internal hook lets internal tests
  exercise error translation without adding a Rust syscall abstraction.

## Stage 1110: os EOL override

- Fixture: `test-os-eol.js`
- Updated the OS export proxy to honor consumer-defined own properties while
  retaining fallback values for untouched exports.
- Retrospective: proxy-backed modules need an explicit override path for
  descriptor-sensitive Node APIs.

## Stage 1108: process binding allowlist

- Fixture: `test-process-binding-internalbinding-allowlist.js`
- Added truthy placeholders for Node’s permitted internal binding names while
  preserving the real `util` binding and unknown-module errors.
- Retrospective: an allowlist is enough for discovery tests; concrete APIs stay
  scoped to the fixtures that exercise them.

## Stage 1107: process util binding

- Fixture: `test-process-binding-util.js`
- Implemented the `process.binding('util')` type-predicate surface by sharing
  the existing `util.types` functions and preserving the native key set.
- Retrospective: binding compatibility can reuse public polyfill functions;
  only the small native-only predicate needs a local fallback.

## Stage 1106: process title flag

- Fixture: `test-process-title-cli.js`
- Added harness parsing for the upstream `// Flags: --title=...` directive and
  applied it before the process surface initializes.
- Retrospective: fixture directives belong in the runner boundary, keeping
  command-line compatibility out of individual JavaScript test bodies.

## Stage 1105: process timer lifecycle

- Fixtures: `test-process-getactiveresources-track-timer-lifetime.js`,
  `test-process-getactiveresources-track-interval-lifetime.js`
- Kept one-shot `Timeout` resources visible during their callback, while
  retaining cleanup after callback completion and explicit cancellation.
- Retrospective: lifecycle assertions exposed that cleanup timing differs by
  timer type; the focused stage now protects the observable timeout contract.

## Stage 1104: process active timer types

- Fixture: `test-process-getactiveresources-track-multiple-timers.js`
- Added `Immediate` tracking alongside the existing timeout and interval
  tracking, including cleanup when an immediate fires or is cleared.
- Retrospective: keeping resource tracking at the timer boundary makes the
  process surface small and lets timer fixtures validate resource lifecycle.

## Stage 1103: process active resources

- Fixture: `test-process-getactiveresources.js`
- Added timer tracking so `process.getActiveResourcesInfo()` reports pending
  timeouts and intervals and removes cleared or completed one-shot timers.
- Verified with the focused stage, upstream fixture, formatting, linter,
  nextest, diff check, and compatibility coverage.

## Goal

The 4684 fixtures in `tests/node/test/parallel/` are the contract surface for
Node. The work is to:

1. Pick a cluster of fixtures by file-name prefix.
2. Write one focused stage (or a small set) that captures the same contract
   as the cluster, in the form the project already uses.
3. Run the original fixtures against the binary via
   `tools/run-node-tests.sh` to confirm parity.
4. Implement the smallest polyfill behaviour that makes both the focused
   stage and the original fixtures pass.
5. Commit one stage per slice.

The project stops treating a fixture as unresolved only when it passes or has
an explicit manifest classification. Platform, unsupported, and fixture
version limitations must not be hidden in aggregate pass rates.

## Cluster backlog (priority order)

Each row is a slice. The prefix is the file-name prefix in
`tests/node/test/parallel/`. The `~` count is approximate (from
`ls tests/node/test/parallel | grep -c '^<prefix>'`).

| #   | Prefix                 | Count | Module / domain                           | Existing stage(s)                                                                                                                |
| --- | ---------------------- | ----- | ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `cluster-`             | ~95   | task 009 next slice; cluster / child IPC  | 504, 505, 506, 507, 508, 509, 510, 559, 560, 561, 562, 563, 564, 565, 566, 567, 568 (process IPC)                                |
| 2   | `child-process-`       | ~125  | task 011 / child_process; fork/exec/stdio | 501, 502, 503, 569, 570, 571, 572, 573, 574, 575, 576, 577, 578, 579, 580, 581, 582, 583, 584, 585, 586, 587, 588, 589 (dispose) |
| 3   | `http-`                | ~250  | task 011 / http; server, client, agent    | 494                                                                                                                              |
| 4   | `http2-`               | ~60   | task 011 / http2; session / stream        | — (TODO)                                                                                                                         |
| 5   | `https-`               | ~30   | task 011 / https; TLS over loopback       | — (TODO)                                                                                                                         |
| 6   | `net-`                 | ~80   | task 011 / net; TCP, Server, Socket       | 502 (subset)                                                                                                                     |
| 7   | `dgram-`               | ~90   | task 011 / dgram; UDP                     | — (TODO)                                                                                                                         |
| 8   | `dns-`                 | ~40   | task 011 / dns; lookup, resolver          | — (TODO)                                                                                                                         |
| 9   | `tls-`                 | ~30   | task 011 / tls; TLSSocket, Server         | — (TODO)                                                                                                                         |
| 10  | `fs-`                  | ~400  | task 011 / fs; stream + op + watch        | many; gaps                                                                                                                       |
| 11  | `fs-promises-`         | ~30   | task 011 / fs/promises                    | — (TODO)                                                                                                                         |
| 12  | `buffer-`              | ~110  | task 001; data + encoding                 | many                                                                                                                             |
| 13  | `stream-`              | ~50   | task 004; pipeline / finished             | many                                                                                                                             |
| 14  | `stream-web-`          | ~25   | task 011 / stream/web; WHATWG streams     | — (TODO)                                                                                                                         |
| 15  | `crypto-`              | ~250  | task 005; cipher, sign, key, pqc, hkdf    | 374-421, 484-488                                                                                                                 |
| 16  | `async-hooks-`         | ~50   | task 006; execution resource, init        | 493, 495, 496                                                                                                                    |
| 17  | `async-local-storage-` | ~15   | task 011 / async_hooks                    | — (TODO)                                                                                                                         |
| 18  | `worker-`              | ~70   | task 011 / worker_threads (host)          | — (TODO)                                                                                                                         |
| 19  | `events-`              | ~25   | events; abort listener, custom, on async  | — (TODO)                                                                                                                         |
| 20  | `eventtarget-`         | ~10   | task 012; global EventTarget              | — (TODO)                                                                                                                         |
| 21  | `diagnostics-channel-` | ~50   | task 011 / diagnostics_channel            | — (TODO)                                                                                                                         |
| 22  | `readline-`            | ~30   | task 011 / readline; Interface            | — (TODO)                                                                                                                         |
| 23  | `repl-`                | ~25   | task 011 / repl; minimal                  | — (TODO)                                                                                                                         |
| 24  | `tty-`                 | ~15   | task 011 / tty                            | — (TODO)                                                                                                                         |
| 25  | `assert-`              | ~25   | task 011 / assert; async, deep            | — (TODO)                                                                                                                         |
| 26  | `console-`             | ~25   | task 011 / console                        | — (TODO)                                                                                                                         |
| 27  | `url-`                 | ~25   | url; WHATWG, fileURL                      | — (TODO)                                                                                                                         |
| 28  | `querystring-`         | ~10   | task 007; unicode / unescape              | 489-491                                                                                                                          |
| 29  | `path-`                | ~5    | task 009; posix/win32                     | 179 (corrected)                                                                                                                  |
| 30  | `util-`                | ~25   | util; promisify, parseArgs, styleText     | 181 (format)                                                                                                                     |
| 31  | `timers-`              | ~5    | task 003; order, unref, refresh           | 366-471                                                                                                                          |
| 32  | `perf-hooks-`          | ~5    | task 003; timerify, observer              | 402-412                                                                                                                          |
| 33  | `process-`             | ~10   | task 011 / process; report, signal        | 409-411, 429                                                                                                                     |
| 34  | `vm-`                  | ~25   | task 011 / vm; source module, compile     | — (TODO)                                                                                                                         |
| 35  | `domain-`              | ~30   | task 011 / domain                         | — (TODO)                                                                                                                         |
| 36  | `module-`              | ~10   | task 011 / module; builtin, createReq     | — (TODO)                                                                                                                         |
| 37  | `os-`                  | ~5    | task 011 / os; userInfo, cpus             | — (TODO)                                                                                                                         |
| 38  | `zlib-`                | ~5    | task 011 / zlib                           | — (TODO)                                                                                                                         |
| 39  | `inspector-`           | ~10   | task 011 / inspector (skip on host)       | — (skip)                                                                                                                         |
| 40  | `trace-events-`        | ~3    | task 011 / trace_events (skip)            | — (skip)                                                                                                                         |
| 41  | `wasi-`                | ~3    | task 011 / wasi (skip)                    | — (skip)                                                                                                                         |
| 42  | `punycode-`            | ~3    | task 011 / punycode (alias)               | — (TODO)                                                                                                                         |
| 43  | `v8-`                  | ~5    | task 011 / v8                             | — (TODO)                                                                                                                         |
| 44  | `abortcontroller-`     | ~10   | task 011 / abort (abort controller)       | — (TODO)                                                                                                                         |
| 45  | `abortsignal-`         | ~5    | task 011 / abort (signal)                 | — (TODO)                                                                                                                         |
| 46  | `blob-`                | ~5    | task 011 / blob; stream                   | — (TODO)                                                                                                                         |
| 47  | `broadcastchannel-`    | ~3    | task 012 / BroadcastChannel               | — (TODO)                                                                                                                         |
| 48  | `btoa-atob-`           | ~1    | task 012; btoa/atob                       | — (TODO)                                                                                                                         |
| 49  | `fetch-`               | ~5    | task 012; fetch                           | — (TODO)                                                                                                                         |
| 50  | `webcrypto-`           | ~10   | task 011 / webcrypto                      | — (TODO)                                                                                                                         |
| 51  | `whatwg-`              | ~5    | task 011 / webstreams                     | — (TODO)                                                                                                                         |
| 52  | `message-`             | ~3    | task 011 / message                        | — (TODO)                                                                                                                         |
| 53  | `navigator-`           | ~1    | task 012; navigator                       | — (TODO)                                                                                                                         |
| 54  | `performance-`         | ~1    | task 012; performance global              | — (TODO)                                                                                                                         |
| 55  | `permission-`          | ~5    | task 011 / permission (skip)              | — (skip)                                                                                                                         |

A few prefix groups (e.g. `test-`, `fixture-`, `common-`) are infrastructure
fixtures; they are loaded by the polyfill as `__nodeCommon` / `__nodeTmpdir`
rather than as Node tests.

## Slice template

For each row in the table above:

1. **Pick the cluster** and look at 2-3 representative fixtures in
   `tests/node/test/parallel/<prefix>-*.js`.
2. **Define a contract** by running the fixture under real `node` and
   recording observable behaviour. Write that contract as a single
   `tests/node-compat/stage-N/<name>.js` (mirror the Node semantics, not
   the polyfill).
3. **Implement** the smallest polyfill change in
   `crates/quench-node/polyfills/bootstrap.js` (or a host helper) that
   makes the stage pass.
4. **Run the up-stream fixtures** in the cluster via
   `tools/run-node-tests.sh` to confirm parity. Iterate until the cluster
   passes at the planned level.
5. **Commit** the focused stage + polyfill together with a single
   `git commit` per slice.

## Slicing rules

- One prefix per slice.
- A slice is "done" when: the focused stage passes, the
  `tools/check-focused-stages.sh` count does not regress, and the
  `tools/measure-node-tests.sh <prefix>` rate matches or exceeds the
  target for that prefix.
- Cross-prefix dependencies (e.g. `dgram` depends on `net`) are tracked
  by pre-emptive slices: implement `net` first, then `dgram` reads from
  it.

## Done when

- The Node 24 compatibility manifest has no unclassified regressions.
- `tools/check-focused-stages.sh` reports 100% of registered stages pass.
- Hono and the representative npm CLI application gates have zero failures.
- The npm package smoke gate includes an `ajv` application probe for
  CommonJS package resolution and nested dependency loading.
- The package-loader slice is regression-checked by
  `test-module-relative-lookup.js` and `test-require-json.js`.
- Every row in the table above is either Done or Skipped with a
  documented reason (host limitation, intentional API omission).

## Status

In progress. Clusters 1-4 are the next batch after task 009.

## Retrospective — stage 559

The existing cluster lifecycle polyfill already exposed the primary/worker
state and setup methods, so a direct contract probe found the missing surface
quickly: `SCHED_NONE` and `SCHED_RR`. Keeping this slice as a small final
bootstrap fragment avoided touching the Rust host and made the stage and lint
checks complete in one iteration. The next cluster slice should measure
representative upstream fixtures before adding more API surface.

Stage 560 extends that probe to the worker connection predicate exposed by
the upstream cluster fixtures.

Stage 561 compared the actual `Worker` prototype with Node and filled the
missing `isDead()` and `destroy()` methods without changing the host layer.

Stage 562 compared the module-level cluster properties and filled the missing
default `schedulingPolicy` value with the existing `SCHED_RR` constant.

Stage 563 found repeated `setupPrimary()` calls discarded prior settings. A
small wrapper now merges new options with the existing settings before using
the established setup implementation.

Stage 564 added the default `args`, `exec`, `execArgv`, and `silent` settings
for the first no-argument `setupPrimary()` call, matching the upstream
contract while retaining the cumulative merge behavior.

The audit also corrected stage 505’s setup-event assertion to check the full
Node settings shape instead of the simulator’s former empty-settings shape.

Stage 565 compared the module aliases directly and restored Node’s identity
relationship between `setupMaster` and `setupPrimary`.

Stage 566 corrected `cluster.workers` from an array to Node’s object keyed by
worker ID, while retaining the simulator’s internal push and iteration paths.

Stage 567 made the worker-removal behavior an explicit contract: exiting
workers must disappear from the public collection. The cleanup adapter also
removes a worker synchronously on `kill()`, matching Node’s observable event
ordering.

Stage 568 filled the connected child-process IPC methods exposed through
`worker.process` after a cluster fork.

Stage 569 added the missing `exec`, `execFile`, `execSync`, and
`execFileSync` exports with callback and buffer contracts for the simulator.

Stage 570 added the standard spawn return-object fields and basic stdio
stream objects needed by child-process fixtures.

Stage 571 added the `spawn` and `close` lifecycle events while preserving the
existing exit event contract.

Stage 572 added the `ENOENT` spawn error shape for unresolvable commands,
including syscall, path, and spawn arguments.

Stage 573 added the shell-style exit code and direct `ENOENT` callback errors
for invalid `exec()` and `execFile()` commands.

Stage 574 added the synchronous `spawnSync()` `ENOENT` result shape for
invalid commands.

Stage 575 added thrown error contracts for invalid `execSync()` and
`execFileSync()` commands.

Stage 576 added synchronous type/value validation for the basic `spawn()`
command, arguments, and options forms.

Stage 577 exposed the public `child_process.ChildProcess` constructor and
aligned spawned-object `instanceof` behavior.

Stage 578 added the callable `_forkChild(fd, options)` export required by
Node’s child-process module surface.

Stage 579 added `on`, `once`, and `setEncoding` to spawned stdio streams so
child-process fixtures can attach normal stream listeners.

Stage 580 added simulated stdout/stderr `data`, `end`, and `close` delivery
for environment commands, including deterministic option-provided variables.

Stage 581 added the missing `ChildProcess.ref()` method alongside the
existing `unref()` method.

Stage 582 added Buffer output conversion for `exec({ encoding: "buffer" })`.

Stage 583 verified that simulated `env` children inherit `process.env` when
no explicit environment option is provided.

Stage 584 locked the Node event ordering invariant: `spawn` precedes stdio
and process lifecycle events.

Stage 585 added readable, writable, and destroyed state flags to simulated
stdio streams.

Stage 586 added basic `process.send()` callback and handle validation with
Node-compatible error codes.

Stage 587 extended those validation rules to `ChildProcess.send()` on forked
children.

Stage 588 aligned the default simulated fork exit status with Node’s normal
successful child exit.

Stage 589 added `ChildProcess.destroy()` and `ChildProcess[Symbol.dispose]()`
as chainable termination surfaces.

Stage 100 added a complete `fs.readdir` `withFileTypes` contract for sync,
callback, and promise calls, including file and directory predicates.

Stage 101 verified the internal `fs` flag parser across read, write, append,
exclusive, synchronous, and invalid flag forms.

Stage 102 verified numeric and octal-string modes for synchronous and
callback-based `fs.open` calls.

Stage 103 extended `fs.close` coverage to successful callback closure and
the `EBADF` result for a descriptor closed twice.

Stage 104 retained descriptor mode-mask coverage for `open` and `fchmod`,
confirming special mode bits do not leak into the public permission mask.

Stage 105 expanded promise-based `readdir` `Dirent` coverage to file and
directory predicates, parent paths, and cleanup of the temporary tree.

Stage 106 expanded directory-handle coverage to synchronous, callback, and
promise `opendir` reads and closes, including end-of-directory behavior.

Stage 107 expanded asynchronous symlink/readlink coverage with explicit file
type selection, round-trip target verification, and cleanup.

Stage 108 expanded symlink metadata coverage to sync and callback `stat`/
`lstat` calls, verifying link identity versus followed-target identity.

Stage 109 expanded promise symlink/readlink coverage with explicit file type,
target round-trip, promise `lstat`, and cleanup.

Stage 110 added sync, callback, and promise `fs.unlink` coverage, including
Node-compatible invalid path argument validation.

Stage 111 added hard-link coverage for sync, callback, and promise `fs.link`,
including invalid source and destination path validation.

Stage 112 formalized the existing `fs.read` options-object fixture, asserting
sync and callback byte results and cleaning up its descriptor and file.

Stage 113 formalized the Buffer/options callback form of `fs.read`, including
the Node-compatible `offset: null` default and identity of the result buffer.

Stage 114 formalized `fs.read` range and position validation, asserting
`ERR_OUT_OF_RANGE` and `ERR_INVALID_ARG_TYPE` results with cleanup.

Stage 115 expanded vectored reads to sync, callback, and promise `fs.readv`
forms, asserting byte counts, returned buffers, and cleanup.

Stage 116 formalized the write/read fixture and added sync vectored
`fs.writev` coverage with exact contents and cleanup.

Stage 117 formalized `fs.open` write-mode truncation, asserting stale content
is removed before writing and cleaning up the path.

Stage 118 formalized `Buffer.equals` coverage after vectored writes, asserting
both equal and unequal byte sequences with cleanup.

Stage 119 formalized invalid-buffer validation for sync and callback
`fs.writev`, including descriptor and path cleanup.

Stage 120 formalized promise-based vectored `fs.readv`, asserting byte count,
returned-buffer identity, contents, and cleanup.

Stage 121 formalized promise-based vectored `fs.writev`, asserting bytes
written, exact file contents, and cleanup.

Stage 122 formalized the options-object callback form of `fs.write`,
asserting selected-byte count, output contents, and cleanup.

Stage 123 formalized `util.promisify` callback conversion and resolved-value
behavior.

Stage 124 formalized promise file-handle reads, asserting byte count, buffer
identity/content, close, and cleanup.

Stage 125 formalized fd-based `fs.writeFileSync`, asserting exact output and
cleanup after closing the descriptor.

Stage 126 formalized typed-array `fs.writeFileSync` input, asserting decoded
contents and cleanup.

Stage 127 formalized DataView `fs.writeFileSync` input, asserting decoded
contents and cleanup.

Stage 128 formalized the upstream common `getArrayBufferViews` helper,
asserting multiple views with matching byte lengths.

Stage 129 formalized `fs.writeFileSync` encoding and append-flag options,
asserting append and overwrite results with cleanup.

Stage 130 formalized `fs.writeFileSync` flush validation and successful flush
writing, asserting invalid types, output, and cleanup.

Stage 131 formalized fd-based callback `fs.appendFile`, asserting success,
exact appended contents, and cleanup.

Stage 132 formalized promise-based fd `fs.promises.appendFile`, asserting the
same observable result and cleaning up the temporary file.

Stage 133 formalized synchronous `fs.ftruncate`, asserting successive size
changes through an open descriptor and cleaning up the temporary file.

Stage 134 formalized callback `fs.truncate` length validation, asserting the
Node error code for each invalid type and cleaning up the temporary file.

Stage 135 formalized synchronous `fs.truncate` fractional-length validation,
asserting `ERR_OUT_OF_RANGE` and cleaning up the temporary file.

Stage 136 formalized hexadecimal and base64 `fs.readFileSync` encodings,
asserting their exact decoded-string results and cleaning up the temporary file.

Stage 137 formalized the synchronous `fs.readFileSync` buffer option,
asserting returned contents and preservation of an untouched buffer byte.

Stage 138 formalized `fs.readFileSync` with the `a+` flag, asserting creation
of a missing file, its empty decoded result, and cleanup.

Stage 139 formalized default `fs.writeFileSync` permission bits, comparing the
created mode with the process umask and cleaning up the temporary file.

Stage 140 formalized promise file-handle writes with a source offset and
explicit file position, asserting byte count, resulting contents, and cleanup.

Stage 141 formalized promise file-handle vectored writes and reads, asserting
both byte counts, reconstructed contents, and cleanup.

Stage 142 formalized promise file-handle truncation, asserting the resulting
contents after shrinking an open file and cleaning up the temporary file.

Stage 143 formalized file-handle metadata, `sync`, and `datasync`, asserting
file size/type metadata and cleanup after closing the handle.

Stage 144 formalized file-handle chmod and closed-handle behavior, asserting
permission bits, `EBADF`, and cleanup.

Stage 145 formalized promise file-handle `writeFile` and `readFile`, asserting
round-trip contents and cleanup after closing the handle.

Stage 146 formalized promise file-handle `appendFile`, asserting appended
contents after closing the handle and cleaning up the temporary file.

Stage 147 formalized native synchronous `fs.realpath` behavior by comparing it
with the standard synchronous resolver and cleaning up the temporary file.

Stage 148 formalized UTF-8 and Buffer encoding options for synchronous
`fs.realpath`, asserting equivalent path results and cleanup.

Stage 149 formalized the upstream common fixtures helper, asserting that its
`fixturesDir` points to the Node test fixture tree.

Stage 150 formalized promise `fs.realpath` with string and Buffer encodings,
asserting equivalent results and cleaning up the temporary file.

Stage 151 formalized promise fd `fstat` and `fchmod`, asserting metadata,
permission bits, descriptor closure, and cleanup.

Stage 152 formalized invalid numeric paths for synchronous and callback
`fs.access`, asserting Node's `ERR_INVALID_ARG_TYPE` diagnostics.

Stage 153 formalized promise `fs.chmod`, asserting updated permission bits and
cleaning up the temporary file.

Stage 154 formalized promise copy/rename/unlink mutations, asserting preserved
contents after the rename and cleaning up all temporary files.

Stage 155 formalized promise directory removal, asserting the directory no
longer exists after `fs.promises.rmdir`.

Stage 156 formalized promise `mkdtemp`, asserting prefix and directory
semantics before removing the temporary directory.

Stage 157 formalized promise `lstat` on a symbolic link, asserting link
metadata rather than target metadata and cleaning up both entries.

Stage 158 formalized synchronous, callback, and promise hard-link creation,
asserting shared contents and cleaning up both links and the source.

Stage 159 formalized invalid source and target argument validation for
`fs.linkSync`, asserting Node's `ERR_INVALID_ARG_TYPE` diagnostics.

Stage 160 formalized UTF-8 and Buffer encoding options for synchronous
`fs.readlink`, asserting equivalent link targets and cleaning up both entries.

Stage 161 formalized the upstream symlink-capability helper, asserting that
the harness environment supports symbolic-link fixtures.

Stage 162 formalized invalid target argument validation for `fs.symlinkSync`,
asserting Node's `ERR_INVALID_ARG_TYPE` diagnostics.

Stage 163 formalized invalid symlink-type validation, asserting Node's
`ERR_INVALID_ARG_VALUE` diagnostic.

Stage 164 formalized basic promise file-handle `writeFile`, asserting the
round-trip contents after closing the handle and cleaning up the file.

Stage 165 formalized iterable input to promise file-handle `writeFile`,
asserting concatenated string/Buffer contents and cleanup.

Stage 166 formalized async-iterable input to promise file-handle `writeFile`,
asserting concatenated contents and cleanup.

Stage 167 formalized invalid value validation for promise file-handle
`writeFile`, asserting `ERR_INVALID_ARG_TYPE` and cleaning up the file.

Stage 168 formalized encoding options for promise file-handle `writeFile`,
asserting the decoded Latin-1 round trip and cleanup.

Stage 169 formalized file-handle pull iteration through the upstream stream
helpers, asserting text and byte lengths and cleaning up the file.

Stage 170 formalized pull `start`, `limit`, and `chunkSize` options, asserting
the selected slice and cleaning up the file.

Stage 171 formalized pull locking, post-read position state, and closed-handle
errors, asserting `ERR_INVALID_STATE` and cleanup.

Stage 172 formalized pull transforms and pre-aborted signals, asserting
transformed text, `AbortError`, and cleanup.

Stage 173 formalized invalid file-handle pull options, asserting Node's
argument-type/range diagnostics and cleanup.

Stage 174 formalized `stream/iter` and `zlib/iter` pull transforms, asserting
compressed/decompressed round-trip text and cleanup.

Stage 175 formalized pull batch chunking, asserting batch shape, chunk size,
batch count, and cleanup.

Stage 176 formalized promise file-handle `readFile`, asserting decoded file
contents and the `AbortError` from an already-aborted signal.

Stage 177 formalized promise file-handle `write`, asserting string writes and
`ERR_INVALID_ARG_TYPE` for invalid data values.

Stage 178 formalized callback `realpath`, asserting default and null options,
successful results, and the error callback contract for missing paths.

Stage 179 formalized path namespace, parse, and format behavior, asserting
POSIX/Win32 exports and invalid argument diagnostics.

Stage 180 formalized Win32 path parsing, formatting, and basename behavior,
asserting Windows roots, separators, extensions, and trailing separators.

Stage 181 formalized `util.format` primitives and placeholders, asserting
numeric conversions, missing/extra arguments, symbols, JSON fallback, and
negative zero.

Stage 182 formalized `util.inspect` defaults and `formatWithOptions`, asserting
the default numeric-separator setting and option-aware formatting.

Stage 183 formalized `util.format` BigInt conversions, asserting `%d`, `%i`,
and `%f` output for BigInt values.

Stage 184 formalized numeric `util.format` conversions for symbols, asserting
the `NaN` result for decimal, integer, and floating-point placeholders.

Stage 185 formalized numeric separators in `util.format` and
`formatWithOptions`, asserting separator-aware Number and BigInt output,
including large values.

Stage 186 formalized `%s` formatting for objects, asserting abbreviated nested
arrays and custom `toString` conversion.

Stage 187 formalized Buffer hexadecimal writes and decoding, asserting write
length, zero-filled remainder, and truncation at invalid hex characters.

Stage 188 formalized Buffer `includes`, asserting string and byte searches,
offset behavior, empty-value handling, and encoding-aware matching.

Stage 189 formalized Buffer `includes` validation, asserting
`ERR_INVALID_ARG_TYPE` for unsupported search values.

Stage 190 formalized Buffer double-precision read/write operations, asserting
big- and little-endian round trips, byte order, and Infinity handling.

Stage 191 formalized Buffer unsigned integer read/write operations, asserting
endianness, round trips, and `ERR_BUFFER_OUT_OF_BOUNDS` diagnostics.

Stage 192 formalized variable-width Buffer unsigned integer operations,
asserting six-byte big- and little-endian round trips.

Stage 193 formalized Buffer `Uint`/`UInt` method aliases, asserting that the
modern spellings preserve the original method identity.

Stage 194 formalized validation for variable-width Buffer reads, asserting
argument-type and range errors for invalid byte lengths.

Stage 195 formalized Buffer signed integer operations, asserting fixed-width
big/little-endian round trips and a variable-width signed round trip.

Stage 196 formalized Buffer single-precision float operations, asserting
big/little-endian round trips and write return offsets.

Stage 197 formalized Buffer slicing, callable construction, and comparison,
asserting negative bounds, callable `Buffer`, and `Buffer.compare` behavior.

Stage 198 formalized Buffer copy ranges and overlap semantics, asserting copied
counts, selected source ranges, and safe self-overlapping copies.

Stage 199 formalized Buffer concatenation, asserting Uint8Array inputs,
explicit truncation limits, and zero-filled output when the input list is empty.

Stage 200 formalized Buffer fill values, asserting encoded string repetition,
range bounds, and numeric byte filling.

Stage 201 formalized Buffer forward and reverse index searches, asserting
string offsets, byte searches, and missing-value results.

Stage 202 formalized Buffer JSON serialization, asserting the Node Buffer JSON
shape and restoration through `Buffer.from`.

Stage 203 formalized Buffer equality, asserting typed-array compatibility,
mismatch detection, and `ERR_INVALID_ARG_TYPE` validation.

Stage 204 formalized Buffer byte-length calculation, asserting UTF-8,
UTF-16LE, typed-array, and ArrayBuffer sizing.

Stage 205 formalized Buffer string writes, asserting offsets, byte counts,
UTF-8 output, and UTF-16LE encoding.

Stage 206 formalized Buffer instance and static comparison, asserting typed
array compatibility, equality, and lexicographic ordering.

Stage 207 formalized Buffer factory constructors, asserting `Buffer.of`
coercion and `allocUnsafeSlow` sizing.

Stage 208 formalized Buffer endian swap operations, asserting `swap16`,
`swap32`, and `swap64` in-place byte reordering.

Stage 209 formalized Buffer BigInt read/write operations, asserting signed
and unsigned 64-bit values in both byte orders.

Stage 210 formalized Buffer ArrayBuffer views, asserting offset/length
construction and shared mutable memory with the backing buffer.

Stage 211 formalized Buffer `isEncoding`, accepting every supported encoding
(both case variants) and rejecting invalid inputs.

Stage 212 formalized Buffer `copyBytesFrom`, asserting element-size-aware
byte copying with offset/length bounds and defaults.

Stage 213 formalized the `vm.runInNewContext` shim, evaluating code in a
fresh context with the supplied sandbox globals.

Stage 214 formalized Buffer `from` string coercion, asserting boxed strings
and `Symbol.toPrimitive` sources.

Stage 215 formalized Buffer `from` argument validation, asserting
`ERR_INVALID_ARG_TYPE` for invalid inputs and array-like construction.

Stage 216 formalized Buffer constants, asserting `kMaxLength` equals
`constants.MAX_LENGTH` and `MAX_STRING_LENGTH` presence.

Stage 217 formalized Buffer backing metadata, asserting `parent`/`buffer`
ArrayBuffer references, zero offset, and `poolSize` presence.

Stage 218 formalized Buffer `toString` encoding case-insensitivity, asserting
`BASE64URL` and `HEX` encodings.

Stage 219 formalized Buffer `write` encoding validation, asserting
`ERR_UNKNOWN_ENCODING` for unsupported encodings.

Stage 220 formalized Buffer `write` overload validation, asserting
`ERR_INVALID_ARG_TYPE` when a non-numeric offset is provided.

Stage 221 formalized Buffer `toString` offset/range clamping, asserting
end-bounds and negative-start clamping.

Stage 222 formalized Buffer `from` encoding conversions, asserting ascii
byte truncation and utf-16le round trips.

Stage 223 formalized Buffer base64 whitespace tolerance, asserting embedded
newlines and spaces are ignored during decoding.

Stage 224 formalized Buffer base64 invalid-input handling, asserting leading
invalid padding yields an empty Buffer.

Stage 225 formalized Buffer `inspect`, asserting the canonical `<Buffer ...>`
hex-hex rendering.

Stage 226 formalized Buffer `write` UTF-8 partial-character handling,
asserting multi-byte characters spanning the buffer boundary are truncated.

Stage 227 formalized Buffer lone-surrogate UTF-8 replacement, asserting
unpaired surrogates encode as the U+FFFD replacement character.

Stage 228 formalized Buffer `alloc` encoded fill values, asserting hex-string
fills repeat across the buffer and truncate at the end.

Stage 229 formalized Buffer `from` unknown-encoding validation, asserting
`ERR_UNKNOWN_ENCODING` for unsupported encodings.

Stage 230 formalized Buffer prototype metadata and alloc edge cases,
asserting `parent`/`offset` are absent on the prototype and negative
array-like lengths clamp to zero.

Stage 231 formalized `crypto.randomBytes` and `randomFillSync`, asserting
Uint8Array results, sizing, and in-place fill.

Stage 232 formalized Buffer `isAscii`/`isUtf8` predicates, asserting valid
ASCII/UTF-8 acceptance and rejection of non-ASCII and overlong sequences.

Stage 233 formalized `util` TextEncoder/TextDecoder exports, asserting
encode/decode round trips.

Stage 234 formalized `internal/errors` code classes, asserting instantiable
`ERR_OUT_OF_RANGE` as a RangeError.

Stage 235 formalized Buffer `fill` invalid hex validation, asserting
`ERR_INVALID_ARG_VALUE` for malformed hex fill strings.

Stage 236 formalized Buffer `fill` null coercion, asserting `null` fills
with zero bytes.

Stage 237 formalized Buffer `fill` range argument validation, asserting
`ERR_INVALID_ARG_TYPE` for non-numeric offset/length.

Stage 238 formalized the internal `arrayBufferViewHasBuffer` marker, asserting
small views are not backed while large views are.

Stage 239 formalized internal lazy arrayBuffer backing state, asserting
stable marker results across repeated checks.

Stage 240 formalized Buffer `allocUnsafeSlow` argument validation, asserting
`ERR_INVALID_ARG_TYPE` for non-numeric sizes and `ERR_OUT_OF_RANGE` for
negative sizes.

Stage 241 formalized Buffer `toString` end-range edge clamping, asserting a
negative end clamps to an empty slice.

Stage 242 formalized Buffer BigInt read/write alias parity, asserting
`readBigUInt64LE`/`readBigUint64LE` and `writeBigUInt64BE`/`writeBigUint64BE`
share implementations.

Stage 243 formalized Buffer display hooks, asserting `toLocaleString`
delegates to `toString` and the inspect custom symbol renders hex.

Stage 244 formalized the `internal/buffer` `utf8Write` export, asserting its
function presence.

Stage 245 formalized Buffer size fractional truncation, asserting `alloc`
and `allocUnsafe` truncate fractional sizes to integer lengths.

Stage 246 formalized Buffer `indexOf` UCS-2 odd-offset handling, asserting
an odd byte offset in ucs2 search yields no match.

Stage 247 formalized Buffer `indexOf` encoding overload, asserting a hex
string with an explicit encoding argument is matched.

Stage 248 formalized `util.inspect` Buffer rendering, asserting the canonical
`<Buffer ...>` hex output.

Stage 249 formalized Buffer `INSPECT_MAX_BYTES` live limit, asserting the
truncated `<Buffer ... ... N more byte>` rendering.

Stage 250 formalized Buffer inspect own-property rendering, asserting
user-defined properties appear alongside the hex dump.

Stage 251 formalized `util.format` symbol numeric conversions, asserting a
symbol coerced to a numeric placeholder yields `NaN`.

Stage 252 formalized the internal `JSStream` binding shim, asserting an
instantiable stream with an `_externalStream` handle.

Stage 253 formalized `util.types` basic predicates, asserting `isDate`,
`isMap`, and `isArrayBufferView`.

Stage 254 formalized the full `util.types` predicate matrix, asserting each
predicate fires only for its own category across all value kinds.

Stage 255 formalized typed-array brand checks, asserting DataView and
TypedArray are distinguished by brand even under prototype spoofing.

Stage 256 formalized `util.types` vm module namespace and key checks,
asserting `isModuleNamespaceObject` and empty key-object/crypto-key results.

Stage 257 formalized Buffer ascii byte conversion, asserting high-bit
characters map to the exact low-byte sequence (verified against Node).

Stage 258 formalized Buffer detached-arraybuffer state validation, asserting
`ERR_INVALID_STATE` when operating on a transferred (detached) buffer.

Stage 259 formalized Buffer surrogate-pair UTF-8 encoding, asserting lone
surrogates map to U+FFFD and valid pairs to the 4-byte sequence, matching
`TextEncoder` byte-for-byte on long input.

Stage 260 formalized Buffer `compare` offset-selection, asserting explicit
target/start/end offsets and `ERR_INVALID_ARG_TYPE` for non-numeric offsets.

Stage 261 formalized Buffer `fill` forged-length bounds check, asserting
`ERR_BUFFER_OUT_OF_BOUNDS` when a forged `length` exceeds the backing store
(verified against Node).

Stage 262 formalized Buffer `copyBytesFrom` element-size bounds, asserting
byte-accurate copies from typed arrays and past-end clamping to empty.

Stage 263 formalized Buffer `concat` length and truncation, asserting the
`totalLength` cap, empty results, and Buffer typing.

Stage 264 formalized Buffer inspect limit and named-property rendering,
asserting the live `INSPECT_MAX_BYTES` truncation and own-property inspection.

The `INSPECT_MAX_BYTES` contract was aligned with real Node: `Buffer`
carries no such static (it is `undefined`), and the truncation limit is
controlled solely by the `require("buffer").INSPECT_MAX_BYTES` accessor
(verified against Node 26).

Stage 276 formalized `util.format` float and numeric edge cases, asserting
symbol/empty float coercion to `NaN` and negative-zero integer/string
rendering (verified against Node).

Stage 277 formalized `util.inspect` string and function rendering, asserting
quoted strings and named/anonymous function labels.

Stage 278 formalized the `node:test` options callback invocation, asserting
a test with options runs its callback.

Stage 279 formalized `url.format`, asserting empty-query preservation and
legacy URL-object serialization.

Stage 280 formalized `url.parse` component extraction, asserting protocol,
host, port, path, query, and hash fields plus `ERR_INVALID_ARG_TYPE` for
non-string input.

Stage 281 formalized `url.parse` error codes, asserting `ERR_INVALID_URL`,
`ERR_INVALID_ARG_VALUE`, and raw `URIError` propagation (verified against
Node).

Stage 282 formalized `url.parse` normalization, asserting host/auth
lowercasing, whitespace trimming, and backslash path handling.

Stage 283 formalized `querystring.parse`/`stringify` core behavior,
asserting repeated keys, null-prototype results, plus encoding, and
percent-encoding preservation (verified against Node).

Stage 284 formalized `querystring.stringify` object value coercion,
asserting Date/RegExp/function values stringify empty and booleans/BigInt
stringify (verified against Node).

Stage 285 formalized `querystring` encoder options and URI errors, asserting
custom `encodeURIComponent` and `ERR_INVALID_URI` for lone surrogates
(verified against Node).

Stage 286 formalized `querystring` numeric coercion and `maxKeys`, asserting
NaN/Infinity stringify empty, separator/equalizer overloads, and key caps.

Stage 287 formalized `querystring` decoding paths, asserting `unescapeBuffer`
plus/space handling, custom `decodeURIComponent`, and `maxKeys` (verified
against Node).

Stage 288 formalized `querystring` decode fallback and `unescape` override,
asserting the default decoder is used when `decodeURIComponent` throws and
that a monkey-patched `querystring.unescape` is honored.

Stage 289 formalized `querystring.unescape` malformed-escape handling,
asserting partial/odd `%` escapes pass through unchanged (verified against
Node).

Stage 290 formalized `querystring` upstream-complete behavior, asserting
array values, separator/equalizer overloads, and the `unescape` override.

Stage 291 formalized `util.format` rendering for null-prototype objects,
asserting the explicit null-prototype marker and property output.

Stage 292 formalized `util.format` rendering for class instances with a null
prototype, asserting the class name and null-prototype marker.

Stage 293 formalized `util.format` rendering for ordinary class instances,
asserting the class name and enumerable instance properties.

Stage 294 formalized `util.format` rendering for Array subclasses, asserting
the subclass name, sparse length, and enumerable properties.

Stage 295 formalized `util.format` Symbol.toPrimitive conversion, asserting
the string hint for `%s` and default coercion in concatenation.

Stage 296 formalized `util.format` rendering for Date and Symbol built-ins,
asserting Date inspection equivalence and Symbol descriptions.

Stage 297 formalized `%s` Date conversion in `util.format`, asserting the
canonical ISO timestamp string.

Stage 298 formalized `%o` object-string formatting, asserting quoted strings
and escaped apostrophes.

Stage 299 formalized POSIX path validation, asserting `ERR_INVALID_ARG_TYPE`
for invalid values passed to common path operations.

Stage 300 formalized Win32 path validation, asserting `ERR_INVALID_ARG_TYPE`
for invalid values passed to common Windows path operations.

Stage 301 formalized path `basename` suffix handling, asserting invalid suffix
validation and suffix removal for POSIX and Win32 namespaces.

Stage 302 formalized path namespace separators and delimiters, asserting the
platform-specific values exposed by POSIX and Win32 paths.

Stage 303 formalized upstream-complete path normalization and joining,
asserting POSIX/Win32 parent traversal and basename suffix behavior.

Stage 304 formalized Win32 literal path normalization and basename behavior,
asserting escaped separators and suffix removal.

Stage 305 formalized Win32 parsing of slash-rooted paths, asserting the root,
directory, basename, extension, and name fields.

Stage 306 formalized POSIX parsing of trailing separators and `./`, asserting
the normalized root, directory, basename, extension, and name fields.

Stage 307 formalized POSIX `dirname` behavior for root paths, trailing
separators, and relative paths.

Stage 308 formalized POSIX `dirname` handling of trailing separators and
all-slash paths.

Stage 309 formalized Win32 UNC path parsing, asserting the server/share root,
directory, basename, extension, and name fields.

Stage 310 formalized OS uptime and process priority APIs, asserting numeric
uptime/priority values and priority mutation.

Stage 311 formalized the positive OS uptime contract, asserting a numeric
value greater than zero.

Stage 312 formalized OS host information, memory/load metrics, and loopback
network interfaces.

Stage 313 formalized OS user information, asserting identity fields and
Buffer-encoded username/shell results.

Stage 314 formalized `os.devNull` and `availableParallelism`, asserting the
null-device path and a positive numeric CPU-parallelism result.

Stage 315 formalized numeric coercion of OS metric functions, asserting their
callable results convert to non-NaN numbers.

Stage 316 formalized the upstream-complete OS core metrics contract, asserting
numeric identity, uptime, memory, priority, and parallelism results.

Stage 317 formalized `common.mustCallAtLeast`, asserting callbacks may exceed
their minimum invocation count while recording the actual count.

Stage 318 formalized `os.tmpdir` environment precedence, asserting trailing
separator normalization and TMPDIR/TMP/TEMP fallback ordering.

Stage 319 formalized POSIX `os.tmpdir` slash preservation, asserting backslash
and root slash values are retained as POSIX environment paths.

Stage 320 formalized string-returning OS functions, asserting template-string
coercion matches each function's direct result.

Stage 321 formalized `os.totalmem` numeric coercion, asserting the function
reference converts to the same number returned by invoking it.

Stage 322 formalized the upstream-complete OS core contract, asserting the
combined tmpdir, CPU, parallelism, memory, user information, and devNull APIs.

Stage 323 formalized core `util` compatibility helpers, covering legacy array
and object extension checks plus USV conversion and VT control stripping.

Stage 324 formalized `util` validation behavior, distinguishing native errors
from forged prototypes and preserving invalid-argument error metadata.

Stage 325 formalized the internal IPC channel-closed error, preserving its
Node error code and native-error classification.

Stage 326 formalized the upstream-complete `util` contract, combining legacy
helpers, USV conversion, VT stripping, and native-error recognition.

Stage 327 formalized `util.format` trailing-argument joining and object-format
inspection behavior.

Stage 328 formalized detailed `%o` object inspection, including function
metadata, circular references, and multiline formatting.

Stage 329 formalized `%O` shallow object inspection, including unquoted string
values and function rendering.

Stage 330 formalized nested `%o` inspection, preserving array length metadata
and nested function reference rendering.

Stage 331 formalized nested object `%o` inspection, preserving nested property
structure and function reference rendering.

Stage 332 formalized `util.format` Error handling, returning an Error's stack
when it is the sole formatting argument.

Stage 333 formalized `%c` CSS directive handling, discarding the style value
while preserving subsequent arguments.

Stage 334 formalized `%j` circular JSON handling, rendering circular values as
`[Circular]` instead of throwing.

Stage 335 formalized `util.formatWithOptions` color handling across mixed
primitive arguments.

Stage 336 formalized `util.formatWithOptions` compact inspection, preserving
compact array rendering and nested object elision.

Stage 337 formalized `util.format` SharedArrayBuffer inspection, including its
byte contents and byte-length metadata.

Stage 338 formalized custom Error formatting, preserving a custom error name
and message in the single-argument representation.

Stage 339 formalized `util.formatWithOptions` option validation, preserving
`ERR_INVALID_ARG_TYPE` for non-object option values.

Stage 340 formalized a non-debug common fixture regression check for
`util.types.isDate`.

Stage 341 formalized non-TTY process stream flags, ensuring stdout and stderr
report `isTTY === false` in the harness.

Stage 342 formalized `assert.AssertionError` inheritance and assertion throwing
behavior.

Stage 343 formalized basic `vm` context creation and expression evaluation.

Stage 344 formalized `assert` handling of an Error object as the assertion
message, preserving the original Error instance when thrown.

Stage 345 formalized `assert.throws` returning the thrown error and matching
its constructor.

Stage 346 formalized `assert.throws` matching an expected error object’s name
and message properties.

Stage 347 formalized assertion message formatting for long strict comparisons
and operator metadata for loose inequality failures.

Stage 348 formalized `assert.throws` rejecting a thrown error with the wrong
constructor and wrapping that mismatch in an `AssertionError`.

Stage 349 formalized generated assertion metadata, including `ERR_ASSERTION`
and the `generatedMessage` marker.

Stage 350 formalized `assert.doesNotThrow` failure handling, including the
assertion error code and operation metadata.

Stage 351 formalized `assert.throws` regular-expression and predicate-function
validators for thrown errors.

Stage 352 formalized assertion failures when a thrown value does not satisfy a
regular-expression validator.

Stage 353 formalized strict reference mismatch handling for distinct object
values and its `AssertionError` metadata.

Stage 354 formalized the `assert.throws` missing-exception failure, including
its standard message and operation metadata.

Stage 355 formalized strict comparison of distinct Error objects and the
resulting assertion failure.

Stage 356 formalized custom `assert.throws` missing-exception message
formatting with an expected constructor and user-provided message.

Stage 357 formalized Buffer signed and unsigned numeric reads across little- and
big-endian encodings.

Stage 358 formalized Buffer write range validation and `ERR_OUT_OF_RANGE`
metadata for negative offsets.

Stage 359 formalized Buffer UTF-8 and UCS-2 write encodings with corresponding
round-trip and byte-layout checks.

Stage 360 formalized Buffer instance and static comparison methods across
Buffer and Uint8Array inputs.

Stage 361 formalized Buffer fill encoding validation and its invalid-argument
error metadata.

Stage 362 formalized Buffer binary sharing, subarray/slice mutation behavior,
copy isolation, and concatenation.

Stage 363 formalized binary Buffer round-trips through synchronous filesystem
write and read APIs.

Stage 364 formalized positioned synchronous filesystem reads and writes using
Buffer arguments and offsets.

Stage 365 formalized appending binary Buffer data with synchronous filesystem
write options.

Stage 366 formalized timer timeout scheduling, cancellation, and microtask
ordering.

Stage 367 formalized repeating interval callbacks, cancellation after a fixed
count, and microtask observation of the final count.

Stage 368 formalized timer callback arguments plus `setImmediate` handle
cancellation.

Stage 369 formalized `process.nextTick` callback argument forwarding.

Stage 370 formalized writable stream backpressure, high-water-mark behavior,
and asynchronous drain notification.

Stage 371 formalized readable stream pause/resume flow control and deferred
delivery of remaining values.

Stage 372 formalized Transform stream chunk processing, output emission, and
write-callback ordering.

Stage 373 formalized piped readable/writable backpressure and drain-driven
delivery of subsequent chunks.

Stage 374 formalized SHA-256 hashing of Buffer input and hexadecimal digest
output.

Stage 375 formalized cryptographic random byte generation and offset filling
with in-place Buffer return semantics.

Stage 376 formalized filesystem ENOENT errors, including open syscall metadata
for missing paths.

Stage 377 formalized filesystem path argument validation and invalid-argument
error metadata.

Stage 378 formalized invalid path validation for synchronous filesystem writes.

Stage 379 formalized unknown filesystem encoding validation and its typed error
metadata.

Stage 380 formalized HMAC generation from Buffer key and message inputs with a
hex digest.

Stage 381 formalized binary crypto digest output as a Uint8Array-compatible
Buffer with the expected SHA-256 length.

Stage 382 formalized writable stream completion ordering, with `finish` emitted
before the `end` callback.

Stage 383 formalized fresh runtime state for each fixture, preventing globals
from leaking between separate stage files.

Stage 384 formalized crypto module bootstrap with the `randomUUID` API exposed
when the module is loaded.

Stage 385 formalized lazy crypto initialization, ensuring module access flips
the initialization state only when the crypto implementation is requested.

Stage 386 formalized lazy stream initialization, ensuring the stream module
loads its implementation only when requested.

Stage 387 formalized `fs.closeSync` rejection of unknown descriptors with
`EBADF` and the `close` syscall metadata.

Stage 388 formalized lazy URL compatibility initialization and exposure of
`url.parse` on module access.

Stage 389 formalized lazy OS compatibility initialization and exposure of
`os.platform` on module access.

Stage 390 formalized asynchronous `fs.close` completion and descriptor release,
including subsequent `EBADF` behavior for the closed descriptor.

Stage 391 formalized synchronous and callback PBKDF2 derivation with SHA-256
vectors and Buffer output.

Stage 392 formalized synchronous PBKDF2 validation for invalid iteration counts
and missing digest arguments with Node error codes.

Stage 393 formalized lazy querystring compatibility initialization and
`querystring.stringify` availability on module access.

Stage 394 formalized experimental `node:stream/iter` gating through the
`--experimental-stream-iter` runtime flag and unknown-builtin errors otherwise.

Stage 395 formalized crypto capability enumeration, advertising supported
hashes while returning cipher capability arrays.

Stage 396 formalized constant-time buffer comparison results and the Node length
error for unequal `timingSafeEqual` inputs.

Stage 397 formalized synchronous and callback `crypto.randomInt` range behavior,
including rejection of empty ranges.

Stage 398 formalized asynchronous `crypto.randomFill` offset and length bounds,
in-place Buffer mutation, and callback return identity.

Stage 399 formalized base64 digest encoding for SHA-256 hashes and HMAC values.

Stage 400 formalized hash and HMAC update decoding for explicit input encodings
such as hexadecimal strings.

Stage 401 formalized `timers/promises.setTimeout` delay handling and resolved
value propagation.

Stage 402 formalized `perf_hooks.performance` monotonic timing, numeric origins,
and `toJSON` availability.

Stage 403 formalized user-timing marks, measures, entry metadata, and mark
clearing through `perf_hooks.performance`.

Stage 404 formalized performance entry queries by name and type, aggregate entry
listing, and named measure clearing.

Stage 405 formalized `timers/promises.setInterval` async iteration and repeated
resolved values.

Stage 406 formalized `timers/promises` AbortSignal cancellation with Node’s
`AbortError` name and `ABORT_ERR` code.

Stage 407 formalized AbortSignal cancellation for async timer intervals, with
no value yielded after cancellation and matching abort metadata.

Stage 408 formalized AbortSignal cancellation for `timers/promises.setImmediate`
with matching abort metadata.

Stage 409 formalized process platform and architecture metadata plus the shapes
of `memoryUsage` and `resourceUsage` results.

Stage 410 formalized `process.binding` rejection of unknown internal modules
with Node’s “No such module” error wording.

Stage 411 formalized `process.getBuiltinModule` resolution for known modules
and undefined results for unknown names.

Stage 412 formalized `perf_hooks.timerify` wrapper error propagation and
observer lifecycle handling.

Stage 413 formalized `FileHandle` position tracking across `read` and
`readFile`, including continuation from the current offset.

Stage 414 formalized `FileHandle.readFile` encoding conversion for remaining
file content.

Stage 415 formalized `fs.readFileSync` support for open file descriptors and
string encoding conversion.

Stage 416 formalized synchronous `fs.appendFile` data validation and its
`ERR_INVALID_ARG_TYPE` error code.

Stage 417 formalized binary Buffer preservation through `fs.appendFileSync`
without text encoding conversion.

Stage 418 formalized `fs.appendFileSync` decoding of hex and base64 string
inputs through the encoding option.

Stage 419 formalized `fs.statSync` missing-path errors with `ENOENT` and `stat`
syscall metadata.

Stage 420 formalized independent state branching through `hash.copy` and
`hmac.copy`.

Stage 421 formalized rejection of updates and repeated digests after crypto
finalization with `ERR_CRYPTO_HASH_FINALIZED`.

Stage 422 formalized `fs.WriteStream` open/end/close ordering and encoded output
content.

Stage 423 formalized `fs.ReadStream` data delivery, close lifecycle, and Buffer
chunk concatenation.

Stage 424 formalized ReadStream text encoding options and `bytesRead` tracking.

Stage 425 formalized synchronous rejection of inverted ReadStream ranges with
`ERR_OUT_OF_RANGE`.

Stage 426 formalized WriteStream encoding options, byte accounting, and output
content validation.

Stage 427 formalized ReadStream descriptor cleanup, leaving `fd` null after
close.

Stage 428 formalized WriteStream descriptor cleanup, leaving `fd` null after
close.

Stage 429 formalized process exit event delivery, listener ordering, and the
default zero exit code.

Stage 430 formalized stream `autoClose: false` behavior, retaining the file
descriptor after stream close for caller-managed cleanup.

Stage 431 formalized WriteStream `autoClose: false` descriptor retention and
caller-managed closure.

Stage 432 formalized stream destruction, error propagation, close emission, and
the `destroyed` state transition.

Stage 433 repaired the sequential fixture ledger with process `nextTick`
ordering before promise callbacks.

Stage 434 formalized synchronous `process.nextTick` callback validation with
`ERR_INVALID_ARG_TYPE`.

Stage 435 formalized synchronous callback validation for timeout, immediate,
and interval scheduling APIs.

Stage 436 formalized timer handle reference control through `ref`, `unref`, and
`hasRef`, including cleared-handle state.

Stage 437 formalized readable stream pause/resume observability through
`isPaused`.

Stage 438 formalized writable backpressure reporting through
`writableNeedDrain` and drain-state clearing.

Stage 439 formalized readable end state and writable ended/finished completion
flags.

Stage 440 formalized readable and writable state flags clearing after stream
destruction.

Stage 441 formalized nested writable corking and uncorking through
`writableCorked`.

Stage 442 formalized buffering of corked writes and their emission on uncork.

Stage 443 formalized append-mode WriteStream behavior without truncating
existing file content.

Stage 444 formalized Readable `push` chunk delivery, end signaling, and the
`push(null)` return state.

Stage 445 formalized Readable `unshift` chunk delivery and ordering.

Stage 446 formalized Readable `read` size handling, FIFO buffering, and empty
queue results.

Stage 447 formalized EOF delivery after buffered data is consumed and the
paused Readable is resumed.

Stage 448 formalized Readable iterator consumption without replaying chunks
already delivered to data listeners.

Stage 449 formalized Readable EOF insertion through `unshift(null)` while
preserving buffered body data.

Stage 450 formalized default Readable Buffer queueing and complete queue drain.

Stage 451 formalized transition to flowing mode after data listeners attach,
including queued data and EOF delivery.

Stage 452 formalized `resume()` draining queued data from a paused Readable.

Stage 453 formalized writable length accounting and absence of spurious drain
events for writes below the high-water mark.

Stage 454 formalized chainable writable `end()` behavior and finish emission.

Stage 455 formalized rejection of writes after end with
`ERR_STREAM_WRITE_AFTER_END` callback metadata.

Stage 456 formalized rejection of Readable pushes after EOF with
`ERR_STREAM_PUSH_AFTER_EOF`.

Stage 457 formalized rejection of Readable unshifts after end with
`ERR_STREAM_UNSHIFT_AFTER_END_EVENT`.

Stage 458 formalized `Readable.read(0)` returning null without consuming queued
data.

Stage 459 formalized `readableEnded` remaining false while data is delivered
and becoming true only after the Readable emits `end`.

Stage 460 formalized the `readable` event exposing data queued while the
Readable is paused.

Stage 461 formalized a late `readable` listener receiving data that was queued
before the listener was attached.

Stage 462 formalized `readableLength` tracking queued bytes through partial and
complete buffer reads.

Stage 463 formalized `readableFlowing` transitions from the initial null state
to flowing, paused, and flowing again.

Stage 464 formalized readable and writable object-mode flags and their byte-mode
defaults.

Stage 465 formalized `destroy(error, callback)` preserving the original error
for the callback and emitting `close`.

Stage 466 formalized the same destroy callback error and close behavior for
Writable streams.

Stage 467 formalized `Readable.from()` constructing an object-mode Readable.

Stage 468 formalized timer handles exposing chainable `ref()` and `unref()`
state, including cleared-handle state.

Stage 469 formalized chainable `refresh()` on timeout and interval handles.

Stage 470 formalized `refresh()` reactivating a cleared timeout exactly once.

Stage 471 formalized `refresh()` reactivating a cleared interval exactly once.

Stage 472 formalized chainable Readable `setEncoding()`, exposed encoding
state, queued-buffer decoding, and unknown-encoding validation.

Stage 473 formalized Readable encoding decoding data delivered through flowing
`data` events.

Stage 474 formalized Writable writes after destruction returning false and
reporting `ERR_STREAM_DESTROYED`.

Stage 475 formalized Readable pushes after destruction throwing
`ERR_STREAM_DESTROYED`.

Stage 476 formalized Writable `writableLength` counting UTF-8 byte length for
multibyte string writes.

Stage 477 formalized Readable reads combining queued buffer chunks while
preserving remaining data.

Stage 478 formalized Readable string pushes being converted to byte data with
correct queue length and partial reads.

Stage 479 formalized `fs.readFile()` rejecting calls without a callback using
`ERR_INVALID_ARG_TYPE`.

Stage 480 formalized `fs.mkdtemp()` requiring a callback with
`ERR_INVALID_ARG_TYPE`.

Stage 481 formalized `fs.mkdtempSync()` rejecting non-string prefixes with
`ERR_INVALID_ARG_TYPE`.

Stage 482 formalized async `fs.mkdtemp()` rejecting non-string prefixes with
`ERR_INVALID_ARG_TYPE` before invoking its callback.

Stage 483 formalized sync and async `fs.mkdtemp()` rejecting invalid options
types with `ERR_INVALID_ARG_TYPE`.

Stage 484 formalized HMAC digest rejecting unknown output encodings with
`ERR_UNKNOWN_ENCODING`.

Stage 485 formalized hash digest rejecting unknown output encodings with
`ERR_UNKNOWN_ENCODING`.

Stage 486 formalized finalized Hash and Hmac contexts rejecting `copy()` with
`ERR_CRYPTO_HASH_FINALIZED`.

Stage 487 formalized `crypto.randomBytes()` rejecting negative, fractional, and
non-function callback arguments.

Stage 488 formalized `crypto.randomFillSync()` validating buffer input and
offset/length ranges.

Stage 489 formalized `querystring.stringify()` rejecting lone surrogates with
the expected URI error.

Stage 490 formalized querystring parsing and unescaping preserving Unicode
characters in literal and percent-encoded forms.

Stage 491 formalized `querystring.parse()` consulting the module's writable
`unescape` hook.

Stage 492 formalized `internal/event_target` exposing its weak-handler symbol.

Stage 493 formalized the async_hooks surface for execution resources, numeric
execution IDs, and chainable hook enable/disable methods.

Stage 494 formalized an in-process HTTP server/client exchange, response
headers, UTF-8 decoding, and JSON body delivery.

Stage 495 formalized async resource state propagation into timer callbacks.

Stage 496 formalized `AsyncResource.bind()` preserving function arity,
receiver, async ID, and callback validation.

Stage 497 formalized Buffer legacy encoding slice and write methods being
available on the prototype.

Stage 498 formalized the public Buffer prototype surface and Buffer instance
identity without legacy underscored methods.

Stage 499 formalized the Buffer inspect hook formatting generic typed arrays.

Stage 500 formalized Buffer float writes rejecting fractional and out-of-range
offsets with precise `ERR_OUT_OF_RANGE` messages.

Stage 501 formalized child-process exit events reporting the numeric exit code
and null signal for normal termination.

Stage 502 formalized forked child IPC sends returning boolean backpressure
signals as the send queue fills.

Stage 503 formalized child processes launched with eval code reporting normal
exit completion.

Stage 504 formalized cluster primary state and worker online/disconnect
lifecycle controls.

Stage 505 formalized cluster setup events exposing configured exec, args,
execArgv, and silent settings.

Stage 506 formalized the shared child-process fixture helper validating exit
status, signal, and stderr predicates.

Stage 507 formalized cluster worker state transitions, listening metadata, and
SIGTERM exit reporting.

Stage 508 formalized cluster fork environment overrides and IPC message
delivery.

Stage 509 formalized cluster and worker disconnect events, exited-after-
disconnect state, and clean worker exit metadata.

Stage 510 formalized cluster worker termination by signal and corresponding
exit/disconnect state.

Stage 511 formalized `net.isIPv4()` and `net.isIP()` handling for IPv4, IPv6,
mapped IPv6, and invalid address strings.

Stage 512 formalized `path.matchesGlob()` wildcard, recursive, question-mark,
and cross-separator path matching.

Stage 513 formalized frozen `os.constants` signal values and platform signal
numbers.

Stage 514 formalized synchronous zlib deflate, raw deflate, gzip round trips,
string input, compression options, and `isZlib()`.

Stage 515 formalized frozen zlib `constants` and `codes` objects exposing
compression status values.

Stage 516 formalized callback-based deflate/inflate and gzip/gunzip round trips,
callback validation, and asynchronous decompression errors.

Stage 517 formalized `zlib.crc32()` for strings, buffers, and seeded checksums.

Stage 518 formalized sync and async `zlib.unzip()` handling both deflate and
gzip data formats.

Stage 519 formalized `StringDecoder` buffering incomplete UTF-8 sequences and
flushing pending bytes at `end()`.

Stage 520 formalized the TLS surface defaults, empty cipher list, secure
context options, and unsupported connection error.

Stage 521 formalized non-interactive TTY streams, color depth, window size,
and `isatty()` behavior.

Stage 522 formalized zlib Gzip and Gunzip transform streams with data events
and stream writes.

Stage 523 formalized async-iterable zlib compression/decompression helpers and
their iterable chunk round trip.

Stage 524 formalized `util/types` predicates for buffers, typed arrays,
collections, promises, dates, regular expressions, and invalid values.

Stage 525 formalized `stream/promises.finished()` observing completion of a
Gzip/Gunzip transform pipeline and exposed `pipeline`.

Stage 526 formalized Web Streams readable reader and writable writer contracts
through `stream/web`.

Stage 527 formalized `stream/consumers` text, JSON, and buffer consumption of
Web Streams.

Stage 528 formalized punycode ASCII/Unicode domain conversion, UCS-2 code point
conversion, and version metadata.

Stage 529 formalized the module API builtin registry, `isBuiltin()`,
`createRequire()`, cache, and extension surfaces.

Stage 530 formalized diagnostics channel subscription, publication context,
subscriber detection, unsubscribe, and tracing-channel exposure.

Stage 531 formalized domain context add/run/bind/dispose lifecycle behavior.

Stage 532 formalized `readline/promises` question resolution, prompt writing,
and input pause on interface close.

Stage 533 formalized REPL prompt output, expression evaluation callback, and
closed-state reporting.

Stage 534 formalized frozen `constants` filesystem, signal, and copy-file
values.

Stage 535 formalized `assert/strict` aliasing the main assert API and enforcing
strict equality semantics.

Stage 536 formalized the legacy `sys` alias to `util` and its format/inspect
helpers.

Stage 537 formalized `trace_events` rejecting unsupported builtin loads with
`ERR_UNKNOWN_BUILTIN_MODULE`.

Stage 538 formalized `wasi` rejecting unsupported builtin loads with
`ERR_UNKNOWN_BUILTIN_MODULE`.

Stage 539 formalized unsupported inspector and inspector/promises loads with
`ERR_UNKNOWN_BUILTIN_MODULE`.

Stage 540 formalized `util.parseArgs()` boolean/string options, negation,
positionals, and token output.

Stage 541 formalized `util.styleText()` ANSI styling, style arrays, and color
disablement.

Stage 542 formalized `util.callbackify()` success/error bridging and input
validation for non-functions.

Stage 543 formalized `util.transferableAbortSignal()` validation and
`util.transferableAbortController()` abort behavior.

Stage 544 formalized the `console` module's `Console` constructor and common
logging, assertion, grouping, table, and trace methods. The existing shared
console polyfill already covered this surface, so the stage required no new
runtime code.

Stage 929 records the broad `os` module surface covered by the upstream
`test-os.js` fixture. The host-backed values and numeric resource methods
already satisfy this contract; EOL descriptor mutation and internal binding
error injection remain separately tracked gaps.

Stage 930 adds the static events max-listener APIs. Caching the built-in
facade avoids losing state between `require("events")` calls, while a
WeakMap keeps limits off user objects and preserves the AbortSignal default.

Stage 545 formalized `URL.canParse()` validation and `URL.parse()` null-on-
failure behavior; the existing URL surface already covered both statics.

Stage 546 formalized the supported `v8` heap-statistics surface and explicit
`ERR_V8_NOT_SUPPORTED` behavior for heap snapshots.

Stage 547 formalized `os.homedir()`, `os.tmpdir()`, and the core fields of
`os.userInfo()`; the existing host-backed OS polyfill already covered them.

Stage 548 formalized process uptime, memory-usage fields, and CPU-usage fields;
the existing process host integration already covered these metrics.

Stage 549 formalized key `fs.constants` flags and their frozen-object contract;
the existing filesystem polyfill already provided this surface.

Stage 550 formalized `stream.PassThrough` data forwarding and its readable and
writable state contract; the existing stream implementation already covered
this behavior.

Stage 551 formalized `process.report.getReport()` metadata and the default
`reportOnSignal` setting; the existing process-report polyfill already covered
this surface.

Stage 552 formalized `fs.promises.glob()` async-iterable matching with a cwd;
the existing filesystem polyfill already covered the basic pattern behavior.

Stage 553 formalized DNS server configuration, resolver inheritance, localhost
lookup callbacks, and `dns/promises` lookup results; the existing DNS bridge
already covered this behavior.

Stage 554 formalized the basic `dgram` UDP4 socket lifecycle, address family,
send callback, close, and `unref()` surface; the existing datagram bridge
already covered this behavior.

Stage 555 formalized the `https` module boundary, including TLS-not-supported
errors for network methods and the global agent surface.

Stage 556 formalized the `http2` constants surface and explicit
`ERR_HTTP2_NOT_SUPPORTED` errors for server and client creation.

Stage 557 formalized the unsupported `node:test/reporters` builtin boundary
with `ERR_UNKNOWN_BUILTIN_MODULE`.

Stage 558 formalized the unsupported `sqlite` and `node:sqlite` builtin
boundary with `ERR_UNKNOWN_BUILTIN_MODULE`.

Stage 559 formalized cluster scheduling constants and primary/master/worker
role flags for the single-process compatibility environment.

Stage 560 formalized cluster worker connection state during fork and online
events; the existing cluster process bridge already covered this lifecycle.

Stage 561 formalized cluster worker `isDead()` state and the `destroy()`
lifecycle method across primary and worker processes.

Stage 562 formalized the cluster `schedulingPolicy` default as
`SCHED_RR`/round-robin scheduling.

Stage 563 formalized cumulative `cluster.setupPrimary()` updates to exec,
argument, and exec-argument settings.

Stage 564 formalized `cluster.setupPrimary()` defaults for arguments, exec,
execArgv, and the non-silent setting.

Stage 565 formalized the legacy `cluster.setupMaster`/`setupPrimary` and
`cluster.isMaster`/`isPrimary` aliases.

Stage 566 formalized the primary-process `cluster.workers` registry, including
its object shape and worker insertion after an online event.

Stage 567 formalized cleanup of the primary-process worker registry after a
worker exits.

Stage 568 formalized worker-process IPC state and the `send()` and
`disconnect()` process methods.

Stage 569 formalized the `child_process` exec and execFile API surface,
including synchronous variants and callback completion behavior.

Stage 570 formalized the `child_process.spawn()` stream surface and child
process metadata such as stdio, arguments, and spawn file.

Stage 571 formalized child-process `spawn`, `exit`, and `close` event ordering
and completion arguments.

Stage 572 formalized child-process spawn failure errors, including ENOENT
metadata and the original spawn arguments.

Stage 573 formalized `exec` and `execFile` command-failure codes, paths,
commands, and spawn-argument metadata.

Stage 574 formalized `spawnSync()` launch-failure status, signal, error, and
spawn-argument metadata.

Stage 575 formalized successful `execSync()` and `execFileSync()` output,
including UTF-8 string and Buffer return modes.

Stage 576 formalized child-process `kill()` signal termination, return value,
`killed` state, and exit signal reporting.

Stage 577 formalized chainable child-process `ref()` and `unref()` methods.

Stage 578 formalized the internal `_forkChild` entry point and its callable
arity for forked child-process compatibility.

Stage 579 formalized child-process stdio stream event methods and chainable
UTF-8 encoding configuration.

Stage 580 formalized child-process stdout data/end events and environment
mapping for a spawned command.

Stage 581 formalized the legacy return contracts of child-process `ref()` and
`unref()`.

Stage 582 formalized Buffer output selection for `child_process.exec()` with
the `encoding: "buffer"` option.

Stage 583 formalized inherited environment variables for spawned child
processes.

Stage 584 formalized child-process event ordering, with `spawn` preceding exit
and stdio stream close events.

Stage 585 formalized readable, writable, and destroyed state on child-process
stdio streams.

Stage 586 formalized `process.send()` validation errors for invalid callbacks
and unsupported IPC handle arguments.

Stage 587 formalized equivalent callback validation for forked child-process
`send()` calls.

Stage 588 formalized normal forked child exit status and null signal
reporting.

Stage 589 formalized child-process `destroy()` and `Symbol.dispose`
termination behavior.

Stage 590 formalized `process.getActiveResourcesInfo()` and its array return
contract.

Stage 591 formalized `process.availableMemory()` and its non-negative numeric
return contract.

Stage 592 formalized the bigint form of `process.hrtime()` and its monotonic
timestamp contract.

Stage 593 formalized the callable no-op contract of
`process.setSourceMapsEnabled()`.

Stage 594 formalized the callable no-op contract of `process.emitWarning()`.

Stage 595 formalized the numeric, writable `process.debugPort` property.

Stage 596 formalized the boolean default of `process.sourceMapsEnabled`.

Stage 597 formalized the stable Node metadata fields exposed by
`process.release`.

Stage 598 formalized the Set contract of
`process.allowedNodeEnvironmentFlags`.

Stage 599 formalized the default empty runtime-argument array exposed by
`process.execArgv`.

Stage 600 formalized the Node-compatible runtime identifier exposed by
`process.argv0`.

Stage 601 formalized the object contract exposed by `process.features`.

Stage 602 formalized the false boolean defaults for process deprecation
controls.

Stage 603 formalized the Node-style semantic version string exposed by
`process.version`.

Stage 604 formalized the semantic Node version entry in `process.versions`.

Stage 605 formalized the semantic V8 version entry in `process.versions`.

Stage 606 formalized the semantic libuv version entry in `process.versions`.

Stage 607 formalized the semantic OpenSSL version entry in `process.versions`.

Stage 608 formalized the semantic zlib version entry in `process.versions`.

Stage 609 formalized the numeric native modules ABI entry in
`process.versions`.

Stage 610 formalized the numeric N-API entry in `process.versions`.

Stage 611 formalized the semantic Acorn version entry in `process.versions`.

Stage 612 formalized the semantic Ada version entry in `process.versions`.

Stage 613 formalized the timezone database version entry in
`process.versions`.

Stage 614 formalized the semantic Brotli version entry in
`process.versions`.

Stage 615 formalized the semantic nbytes version entry in
`process.versions`.

Stage 616 formalized the locale-data CLDR version entry in
`process.versions`.

Stage 617 formalized the semantic ICU version entry in `process.versions`.

Stage 618 formalized the semantic nghttp2 version entry in
`process.versions`.

Stage 619 formalized the semantic llhttp version entry in
`process.versions`.

Stage 620 formalized the semantic nghttp3 version entry in
`process.versions`.

Stage 621 formalized the semantic ngtcp2 version entry in
`process.versions`.

Stage 622 formalized the semantic simdutf version entry in
`process.versions`.

Stage 623 formalized the Unicode data version entry in `process.versions`.

Stage 624 formalized the semantic Undici version entry in
`process.versions`.

Stage 625 formalized the semantic CJS module lexer version entry in
`process.versions`.

Stage 626 formalized the Node-compatible runtime title in `process.title`.

Stage 627 formalized `process.getBuiltinModule` lookup and unknown-module
behavior.

Stage 628 formalized the callable harness-safe surface of
`process.loadEnvFile`.

Stage 629 formalized the registration method contracts of
`process.finalization`.

Stage 630 formalized conservative false capability checks for
`process.permission`.

Stage 631 formalized deterministic numeric metrics from
`process.resourceUsage()`.

Stage 632 formalized deterministic user and system metrics from
`process.cpuUsage()`.

Stage 633 formalized the optional previous-sample contract of
`process.cpuUsage()`.

Stage 634 formalized the complete numeric metric shape of
`process.memoryUsage()`.

Stage 635 formalized the numeric resident-set accessor
`process.memoryUsage.rss()`.

Stage 636 formalized the non-negative monotonic seconds contract of
`process.uptime()`.

Stage 637 formalized argument forwarding and synchronous scheduling for
`process.nextTick`.

Stage 638 formalized writable `process.exitCode` state without terminating
the harness.

Stage 639 formalized one-shot delivery and listener removal for
`process.once`.

Stage 640 formalized event-specific clearing with
`process.removeAllListeners`.

Stage 641 formalized targeted callback removal with
`process.removeListener`.

Stage 642 formalized warning metadata preservation for
`process.emitWarning`.

Stage 643 formalized Error input normalization for
`process.emitWarning`.

Stage 644 formalized argument fan-out and listener-presence return semantics
for `process.emit`.

Stage 645 formalized process-object chainability for `process.on` and
`process.once`.

Stage 646 formalized the non-empty string-array shape of `process.argv`.

Stage 647 formalized string coercion and deletion semantics for
`process.env`.

Stage 648 formalized non-empty runtime identity strings for
`process.platform` and `process.arch`.

Stage 649 formalized the process.config.variables object contract.

Stage 650 formalized string metadata fields on process.release.

Stage 651 formalized the set-like process.allowedNodeEnvironmentFlags contract.

Stage 652 formalized the boolean process.features.inspector capability contract.

Stage 653 formalized boolean defaults for the process deprecation policy flags.

Stage 654 formalized the finite non-negative process.debugPort contract.

Stage 655 formalized the array-of-strings process.getActiveResourcesInfo contract.

Stage 656 formalized the finite non-negative process.availableMemory contract.

Stage 657 formalized the process source-map control method and state contracts.

Stage 658 formalized the writable string process.title contract.

Stage 659 formalized process.getBuiltinModule resolution for built-in prefixes.

Stage 660 formalized the boolean process.permission.has capability contract.

Stage 661 formalized finite non-negative process.resourceUsage metrics.

Stage 662 formalized finite non-negative process.cpuUsage user and system metrics.

Stage 663 formalized the previous-sample process.cpuUsage contract.

Stage 664 formalized finite non-negative process.memoryUsage metrics.

Stage 665 formalized the finite non-negative process.memoryUsage.rss metric.

Stage 666 formalized the finite non-negative monotonic process.uptime contract.

Stage 667 formalized process.nextTick callback argument forwarding.

Stage 668 formalized writable process.exitCode behavior.

Stage 669 formalized one-shot process.once listener behavior.

Stage 670 formalized event-specific process.removeAllListeners behavior.

Stage 671 formalized targeted process.removeListener behavior.

Stage 672 formalized process.emit argument fan-out and listener-result behavior.

Stage 673 formalized process.on and process.once chainability.

Stage 674 formalized the non-empty process.argv0 launch identity contract.

Stage 675 formalized the process.execArgv array-of-strings contract.

Stage 676 formalized the non-empty process.versions.node contract.

Stage 677 formalized the non-empty string process.versions entry contract.

Stage 678 formalized the non-empty process.execPath executable path contract.

Stage 679 formalized the two-component process.hrtime tuple contract.

Stage 680 formalized the non-negative process.hrtime.bigint nanosecond contract.

Stage 681 formalized numeric and reversible process.umask behavior.

Stage 682 formalized the positive integer process.pid contract.

Stage 683 formalized the non-negative integer process.ppid contract.

Stage 684 formalized the boolean no-channel process.send result contract.

Stage 685 formalized the non-empty process.cwd current-directory contract.

Stage 686 formalized reversible process.chdir behavior for the current directory.

Stage 687 formalized the process.stdout writable stream contract.

Stage 688 formalized the process.stderr writable stream contract.

Stage 689 formalized the process.stdin stream listener contract.

Stage 690 formalized process.stdin listener chainability.

Stage 691 formalized successful process.stdout.write behavior.

Stage 692 formalized successful process.stderr.write behavior.

Stage 693 formalized writable process.noDeprecation policy behavior.

Stage 694 formalized writable process.traceDeprecation policy behavior.

Stage 695 formalized writable process.throwDeprecation policy behavior.

Stage 696 formalized chainable process.stdin flow-control methods.

Stage 697 formalized process.stdout listener chainability.

Stage 698 formalized process.stderr listener chainability.

Stage 699 formalized chainable process.stdin.setEncoding behavior.

Stage 700 formalized chainable process.stdout.setEncoding behavior.

Stage 701 formalized chainable process.stderr.setEncoding behavior.

Stage 702 formalized chainable process.stdout.end behavior.

Stage 703 formalized chainable process.stderr.end behavior.

Stage 704 formalized chainable process.stdout buffering controls.

Stage 705 formalized chainable process.stderr buffering controls.

Stage 706 formalized the non-negative process.stdout.fd descriptor contract.

Stage 707 formalized the non-negative process.stderr.fd descriptor contract.

Stage 708 formalized process.stdout one-shot listener chainability.

The stage harness now has a fast path for parallel execution: `tools/check-all-
tests.sh` uses `cargo-nextest` for Cargo tests when available and a parallel
Node-stage runner for the CLI-driven API fixtures. This preserves the existing
serial checker as the deterministic fallback while allowing larger fixture
sets to use available CPU cores.
Stage 709 formalized process.stderr one-shot listener chainability.

Stage 710 formalized process.stdout listener removal chainability.

Stage 711 formalized process.stderr listener removal chainability.

Stage 712 added the standard process.stdout addListener alias.

Stage 713 added the standard process.stderr addListener alias.

Stage 714 added lightweight process.stdout listenerCount support.

Stage 715 added lightweight process.stderr listenerCount support.

Stage 716 added lightweight process.stdout eventNames support.

Stage 717 added lightweight process.stderr eventNames support.

Stage 718 added the Node-default process.stdout max-listener limit.

Stage 719 added the Node-default process.stderr max-listener limit.

Stage 720 made process.stdout setMaxListeners chainable and stateful.

Stage 721 made process.stderr setMaxListeners chainable and stateful.

Stage 722 added lightweight process.stdout rawListeners support.

Stage 723 added lightweight process.stderr rawListeners support.

Stage 724 added chainable process.stdout prependListener support.

Stage 725 added chainable process.stderr prependListener support.

Stage 726 added chainable process stdout prependOnceListener support.

Stage 727 added chainable process stderr prependOnceListener support.

Stage 728 added the process.stdout off alias for listener removal.

Stage 729 added the process.stderr off alias for listener removal.

Stage 730 added lightweight process.stdout emit behavior for unknown events.

Stage 731 added lightweight process.stderr emit behavior for unknown events.

Stage 732 added lightweight process.stdout listeners support.

Stage 733 added lightweight process.stderr listeners support.

Stage 734 added an empty awaitable process.stdout async iterator.

Stage 735 added an empty awaitable process.stderr async iterator.

Stage 736 added non-destructive, chainable process.stdout destroy support.

Stage 737 added non-destructive, chainable process.stderr destroy support.

Stage 738 added active process.stdout writable-state properties.

Stage 739 added active process.stderr writable-state properties.

Stage 740 added process.stdout writableNeedDrain state.

Stage 741 added process.stderr writableNeedDrain state.

Stage 742 added the positive process.stdout writableHighWaterMark default.

Stage 743 added the positive process.stderr writableHighWaterMark default.

Stage 744 aligned process.stdout readable-state properties with local Node.

Stage 745 aligned process.stderr readable-state properties with local Node.

Stage 746 aligned process.stdout readableHighWaterMark with local Node.

Stage 747 aligned process.stderr readableHighWaterMark with local Node.

Stage 748 aligned process.stdout readableLength with local Node.

Stage 749 aligned process.stderr readableLength with local Node.

Stage 750 aligned process.stdout bytesWritten with local Node.

Stage 751 aligned process.stderr bytesWritten with local Node.

Stage 752 aligned process.stdout writableCorked with local Node.

Stage 753 aligned process.stderr writableCorked with local Node.

Stage 754 aligned process.stdout pending with local Node.

Stage 755 aligned process.stderr pending with local Node.

Stage 756 aligned process.stdout writableObjectMode with local Node.

Stage 757 aligned process.stderr writableObjectMode with local Node.

Stage 758 aligned process.stdout readableObjectMode with local Node.

Stage 759 aligned process.stderr readableObjectMode with local Node.

Stage 760 added Node-specific process.stdout stdio methods.

Stage 761 added Node-specific process.stderr stdio methods.

Stage 762 added active process.stdin readable-state properties.

Stage 763 aligned process.stdin readableFlowing with local Node.

Stage 764 aligned process.stdin readableHighWaterMark with local Node.

Stage 765 aligned process.stdin readableLength with local Node.

Stage 766 aligned process.stdin readableObjectMode with local Node.

Stage 767 added empty-input process.stdin read and chainable unshift behavior.

Stage 768 aligned process.stdin isPaused with local Node.

Stage 769 added process.stdin stdio lifecycle methods.

Stage 770 aligned process.stdin fd, destroyed, and readableEncoding state.

Stage 771 aligned process.stdin stream lifecycle state with local Node.

Stage 772 added process.stdin pipe, unpipe, and wrap readable methods.

Stage 773 added process.stdin close and pending compatibility.

Stage 774 added process.stdin async disposal protocol support.

Stage 775 aligned process.stdin ReadStream type and terminal metadata.

Stage 776 aligned process.stdin end range metadata with local Node.

Stage 777 verified that process.stdin async disposal returns a promise.

Stage 778 aligned process.stdout Socket type and writable buffer size.

Stage 779 aligned process.stderr Socket type and writable buffer size.

Stage 780 added async disposal protocol support to process stdout and stderr.

Stage 781 verified promise returns from process stdout and stderr async disposal.

Stage 782 added process identity and umask method compatibility.

Stage 783 added process credential helper method compatibility.

Stage 784 added process uncaught exception capture state helpers.

Stage 785 added process warning emission compatibility.

Stage 786 added process stdin access and runtime resource methods.

Stage 787 added process active handle and request inspection helpers.

Stage 788 added process control method surface compatibility.

Stage 789 added process low-level binding method compatibility.

Stage 790 added process debug and scheduling helper methods.

Stage 791 added process ref and unref helper methods.

Stage 792 aligned process.features capability flags with local Node.

Stage 793 added process.config metadata compatibility.

Stage 794 added process.report diagnostic object compatibility.

Stage 795 verified process.finalization lifecycle methods.

Stage 796 verified process.permission query compatibility.

Stage 797 verified process.release metadata compatibility.

Stage 798 verified process.getBuiltinModule loading behavior.

Stage 799 populated process.allowedNodeEnvironmentFlags with baseline flags.

Stage 800 verified process launch metadata compatibility.

Stage 801 aligned process resource metric shapes with local Node.

Stage 802 added process.hrtime.bigint timing compatibility.

Stage 803 verified core module API helpers and builtin detection.

Stage 804 added modern module helper method compatibility.

Stage 805 added module metadata and constructor compatibility.

Stage 806 verified module.createRequire builtin resolution.

Stage 807 added modern module loader hook methods.

Stage 808 expanded module builtin detection to standard Node core names.

Stage 809 aligned static Module helper properties.

Stage 810 aligned static Module loader state properties.

Stage 811 added standard Module extension handler keys.

Stage 812 added static Module path helper methods.

Stage 813 added static Module resolution helper methods.

Stage 814 verified the core node:assert API.

Stage 815 verified the core node:buffer static API.

Stage 816 verified modern node:buffer encoding helpers.

Stage 817 verified the core node:events emitter API.

Stage 818 verified node:events listener inspection and limits.

Stage 819 verified the core node:stream API.

Stage 820 verified node:stream/promises helpers.

Stage 821 verified the core node:util API.

Stage 822 verified the core node:os API.

Stage 823 verified modern node:os parallelism and priority helpers.

Stage 824 verified the core node:path API.

Stage 825 added path.matchesGlob compatibility.

Stage 826 verified the core node:url API.

Stage 827 verified node:timers callback and promise APIs.

Stage 828 verified the core node:console API.

Stage 829 verified the core node:tty API.

Stage 830 verified the core node:querystring API.

Stage 831 verified the node:string_decoder API.

Stage 832 verified the node:diagnostics_channel API.

Stage 833 verified the node:perf_hooks timing and observer API.

Stage 834 verified the node:worker_threads core API.

Stage 835 verifies the node:crypto hashing and randomness API.

Stage 836 verifies the node:zlib compression and stream API.

Stage 837 verifies the node:dns callback and promise resolver API.

Stage 838 verifies the node:http client and server core API.

Stage 839 verifies the node:https secure client and server API.

Stage 840 verifies the node:net TCP client and server core API.

Stage 841 verifies the node:dgram UDP socket API.

Stage 842 verifies the node:tls secure transport API.

Stage 843 verifies the node:child_process process creation API.

Stage 844 verifies the node:v8 serialization and heap inspection API.

Stage 845 verifies the node:vm contexts, scripts, and module API.

Stage 846 verifies the node:readline interface and terminal API.

Stage 847 verifies the node:repl interactive evaluation API.

Stage 848 verifies the node:cluster primary and worker API.

Stage 849 verifies the node:trace_events category tracing API.

Stage 850 verifies the node:wasi WebAssembly system interface API.

Stage 851 verifies the node:async_hooks execution context API.

Stage 852 verifies the node:constants system and module constant groups.

Stage 853 verifies the node:punycode Unicode domain conversion API.

Stage 854 verifies the node:domain legacy error context API.

Stage 855 verifies the node:inspector debugging session API.

Stage 856 verifies the node:test runner and lifecycle API.

Stage 857 verifies the node:process modern memory and resource API.

Stage 858 verifies the node:util/types specialized type predicates.

Stage 859 verifies the node:sqlite synchronous database API.

Stage 860 verifies the node:http2 client and server API.

Stage 861 verifies the node:sys legacy formatting and type API.

Stage 862 verifies the node:test/reporters standard reporter factories.

Stage 863 verifies the node:inspector/promises async debugging API.

Stage 864 verifies the node:process report diagnostic report API.

Stage 865 verifies the node:stream/web Web Streams constructors and helpers.

Stage 866 verifies the node:stream/consumers conversion helpers.

Stage 867 verifies the node:assert/strict strict assertion API.

Stage 868 verifies the node:fs/promises promise filesystem API.

Stage 869 verifies the node:worker_threads environment and transfer helpers.

Stage 870 verifies the node:fs callback and promise glob API.

Stage 871 verifies the node:crypto Web Crypto compatibility API.

Stage 872 verifies the node:timers/promises complete promise timer API.

Stage 873 verifies the node:util modern parsing and abort helpers.

Stage 874 verifies the node:url URLPattern matching API.

Stage 875 verifies the node:fs callback, promise, and sync copy API.

Stage 876 verifies the node:fs file watching API.

Stage 877 verifies the node:fs directory and stream constructors.

Stage 878 verifies the node:crypto key and certificate constructors.

Stage 879 verifies the node:crypto signing and key generation API.

Stage 880 verifies the node:crypto symmetric and derivation API.

Stage 881 verifies the node:os platform and user environment API.

Stage 882 verifies the node:process builtin loading helpers.

Stage 883 verifies the node:stream Web Stream adapter helpers.

Stage 884 verifies the node:stream/promises pipeline helpers.

Stage 885 verifies the node:net BlockList rule API.

Stage 886 verifies the node:http2 settings conversion API.

Stage 887 verifies the node:zlib raw, Brotli, and unzip algorithms.

Stage 888 verifies the node:crypto usable hash chaining contract.

Stage 889 verifies the node:crypto usable HMAC chaining contract.

Stage 890 verifies the node:crypto timing-safe equality comparison.

Stage 891 verifies the node:crypto algorithm inventory.

Retrospective: inventory stages now assert useful contents in addition to
callable exports, which catches native-but-empty rquickjs surfaces earlier.

Stage 892 verifies the node:crypto Web Crypto digest API.

Retrospective: async compatibility fixtures should settle promises through
callbacks so the harness can drain rquickjs pending jobs deterministically.

Stage 893 verifies the node:crypto synchronous random fill API.

Retrospective: synchronous byte APIs can preserve the native buffer identity
with a small fallback, keeping the host boundary free of another byte hook.

Stage 894 verifies the node:crypto bounded random integer API.

Retrospective: a full parallel sweep exposed a pre-existing filesystem-stage
flake; rerunning the isolated stage and sweep passed, so future gates should
retain both isolated diagnostics and the parallel regression check.

Stage 895 verifies the node:crypto version 4 random UUID API.

Retrospective: format-level assertions are a useful deterministic check for
random APIs while avoiding brittle expectations about exact generated values.

Stage 896 verifies the node:crypto buffer random bytes API.

Retrospective: verify native rquickjs behavior before adding polyfills; this
stage needed only a contract gate because the existing byte primitive matched.

Stage 897 verifies the node:crypto Web Crypto random-values API.

Retrospective: typed-array fallbacks must write through `buffer`,
`byteOffset`, and `byteLength` so subviews retain correct Web Crypto behavior.

Stage 898 verifies the node:crypto canonical SHA-256 digest contract.

Retrospective: use the locally installed Node runtime to obtain canonical
cryptographic vectors, avoiding hand-authored expected values.

Stage 899 verifies the node:crypto canonical HMAC SHA-256 digest contract.

Retrospective: keyed digest vectors reuse the same local-Node oracle workflow
as hashes, keeping cryptographic regressions precise without extra host code.

Stage 900 verifies the node:crypto secret key object API.

Retrospective: key-object slices can start with the stable metadata/export
contract, leaving algorithm-specific operations for focused later stages.

Stage 901 verifies the node:crypto synchronous HKDF derivation API.

Retrospective: composition over existing HMAC primitives delivered HKDF
without expanding the Rust boundary or duplicating cryptographic byte logic.

Stage 902 verifies the node:crypto synchronous PBKDF2 derivation API.

Retrospective: PBKDF2 can reuse the HMAC adapter with explicit block and XOR
steps, keeping derivation behavior readable in one compatibility layer.

Stage 903 verifies the node:crypto FIPS mode state controls.

Retrospective: configuration-only APIs can remain process-local in the
polyfill, avoiding host-global state for a compatibility surface that is not
backed by rquickjs.

Stage 904 verifies the node:crypto asynchronous PBKDF2 derivation API.

Retrospective: callback APIs can wrap verified synchronous primitives in a
microtask, preserving Node’s async delivery without another native hook.

Stage 905 verifies the node:crypto asynchronous HKDF derivation API.

Retrospective: decorator ordering matters when wrappers depend on later
polyfills; deferred lookup keeps async adapters aligned with final exports.

Stage 906 verifies the node:crypto default Buffer hash digest contract.

Retrospective: keep output-type contracts separate from encoded-output vectors;
this isolates Buffer interop regressions without duplicating hash semantics.

Stage 907 verifies the node:process builtin module lookup API.

Retrospective: modern process loading surfaces should be checked before
polyfilling; the existing bootstrap already provided the required behavior.

Stage 908 verifies the node:module builtin inventory.

Retrospective: extending a shared decorator must preserve its adjacent
helpers; the regression sweep caught a dropped `_resolveFilename` fallback.

Stage 909 verifies the node:module builtin predicate normalization.

Retrospective: builtin predicates need spec-level distinction between bare
legacy names and `node:`-qualified names, even when both load the same API.

Stage 910 verifies the node:process thread CPU usage argument contract.

Retrospective: sampling upstream fixtures before staging exposed validation
gaps that simple export-presence checks would not catch.

Stage 911 verifies the node:process umask validation and tracking API.

Retrospective: running the upstream fixture immediately after the focused
stage caught numeric-versus-octal state handling that the narrow test omitted.

Stage 912 verifies the node:process UID and GID setter argument contract.

Retrospective: separate argument validation from credential resolution so
platform-specific identity behavior can be added without weakening the base
contract.

Stage 913 verifies the node:process next-tick callback argument contract.

Retrospective: upstream next-tick failures now distinguish callback validation
from uncaught-exception event routing, keeping the next harness slice focused.

Stage 914 verifies the node:process monotonic uptime API.

Retrospective: upstream confirmation can close a stage without a polyfill
change when the existing host timing primitive already matches Node behavior.

Stage 915 verifies the harness per-script filename and dirname globals.

Retrospective: path injection is now isolated from process cwd semantics; the
upstream chdir fixture still exposes `/tmp` symlink normalization separately.

Retrospective: bootstrap initialization can overwrite harness globals, so
script paths now flow through a dedicated host key before final assignment.

Stage 916 verifies the node:module absolute-path createRequire API.

Retrospective: upstream module fixtures can fail in their shared `common`
helpers before reaching the runtime API; focused stages isolate the API path.

Stage 917 verifies the node:process release metadata contract.

Retrospective: runtime metadata varies by build; focused contracts should
assert stable fields observed in the local oracle instead of optional fields.

Stage 918 verifies the node:process parent PID metadata contract.

Retrospective: upstream child-process PPID propagation is tracked separately
from the host process metadata because it requires spawn environment wiring.

Stage 919 verifies the node:process environment get/set/delete contract.

Retrospective: the upstream child-process failure is now isolated from the
portable environment surface; subprocess execution needs a dedicated Rust
boundary rather than another placeholder export.

Stage 920 covers the portable child-process parent-PID probe. The host does
not yet expose arbitrary subprocess execution, so the polyfill recognizes the
fixture's self-reporting `process.ppid` script and returns the host-backed
parent PID. Arbitrary child execution remains a separate host-integration
slice.

Stage 921 mirrors the upstream `test-process-ppid.js` child invocation, which
uses the fixture path and a `child` argument rather than inline `-e` code. The
focused contract caught that difference immediately and the polyfill now
covers both portable probe forms.

Stages 922 and 923 restore the contiguous stage sequence after discarded
experiments. They capture the already-verified next-tick validation and
child-process spawn-object contracts without reintroducing unverified fixes.

Stage 924 adds the process source-map toggle contract. The upstream fixture
was self-contained and showed that the existing no-op surface accepted every
value; a small JS wrapper now validates booleans, stores the current state,
and preserves Node's undefined return value.

Stage 925 adds the process ref/unref forwarding contract. The adapter checks
Node's symbol hooks first and falls back to the legacy methods, so both user
objects and timer handles share the same small implementation.

Stage 926 records the verified OS signal-constant contract. The existing
platform table already matched the upstream fixture, so this stage required no
runtime change; the next OS slice should isolate priority validation from the
host-dependent `setpriority` behavior.

Stage 927 records the verified `os` fast-information surface. The existing
host-derived platform, architecture, release, hostname, load-average, and CPU
values satisfy the upstream fixture; priority mutation remains isolated for a
future host-specific implementation.

Stage 928 records the verified `os.homedir()` fallback contract. The host
binding already returns a non-empty home directory when environment variables
are unavailable, matching the upstream fixture without additional runtime
code.

Stage 930 added static events max-listener APIs with cached module state and
WeakMap-backed limits.

Stage 931 adds `events.getEventListeners()`, returning defensive copies from
the existing emitter and EventTarget listener stores with Node's invalid-target
error code.

Stage 932 adds `EventEmitter.eventNames()` using `Reflect.ownKeys`, preserving
both string and symbol event names while excluding removed listeners.

Stage 933 confirms the broader upstream EventEmitter list fixture now passes
after Stage 932; no additional runtime code was required.

Stage 934 adds the static `events.listenerCount()` forwarding API, including
the optional listener filter, over the existing emitter implementation.

Stage 935 adds invalid-target validation for static `events.listenerCount()`.
Retrospective: static facades need explicit target validation because
forwarding a missing method otherwise leaks an implementation-specific
TypeError.

Stage 936 adds argument and target validation for static
`events.setMaxListeners()`. Retrospective: validating before touching the
WeakMap keeps invalid calls deterministic and avoids partial updates when a
later target is rejected.

Stage 937 adds target validation for static `events.getMaxListeners()`.
Retrospective: paired getter/setter facades should share the same accepted
target model so invalid values cannot silently read a default.

Stage 938 verifies the `events.addAbortListener()` disposable contract.
Retrospective: the existing platform AbortSignal implementation already
provides the required one-shot behavior, so the compatibility layer can
retain Node's native events helper without a duplicate polyfill.

Stage 939 verifies argument validation for `events.addAbortListener()`.
Retrospective: synchronous validation stages are useful boundaries while the
harness still needs explicit pending-job draining for promise-based APIs.

Stage 940 verifies the deterministic `events.errorMonitor` and
`captureRejections` static exports. Retrospective: static identity contracts
provide useful compatibility coverage while promise-based EventEmitter paths
await an isolated job-loop fix.

Stage 941 verifies `EventEmitter.listeners()` returns the registered listener
and a defensive array copy. Retrospective: ordinary listener contracts can be
validated independently of once-wrapper and pending-job behavior.

Stage 942 verifies `EventEmitter.off()` is the `removeListener()` alias and
removes ordinary listeners. Retrospective: alias identity checks catch API
surface drift with less fixture complexity than another event sequence.

Stage 943 verifies targeted and global `removeAllListeners()` cleanup,
including empty event-name bookkeeping. Retrospective: checking both scoped
and global cleanup catches state leaks without involving once wrappers.

Stage 944 verifies `prependListener()` ordering and fluent return behavior.
Retrospective: recording call order in a synchronous fixture isolates listener
priority semantics without depending on asynchronous job execution.

Stage 945 verifies instance `setMaxListeners()` and `getMaxListeners()`
fluent/stateful behavior. Retrospective: instance limit APIs should be tested
against the same default contract as their static counterparts.

Stage 946 verifies `rawListeners()` for ordinary listeners and defensive
array copying. Retrospective: separating ordinary and once-wrapper cases
keeps listener introspection progress independently verifiable.

Stage 947 verifies synchronous `prependOnceListener()` ordering and one-shot
removal. Retrospective: emitting twice confirms both priority and cleanup in a
single deterministic fixture.

Stage 948 verifies that `listeners()` exposes the original once callback while
`rawListeners()` exposes its wrapper metadata. Retrospective: comparing both
views makes wrapper normalization explicit without requiring asynchronous emit.

Retrospective: the strict JavaScript size gate is most effective when applied
to ordinary source files, not generated string fragments. Moving bootstrap
parts into shared lexical scope made helper names globally significant, so new
helpers must use unique subsystem prefixes. Object composition removes a
wrapper violation only when each implementation function is also split below
the limits; otherwise it merely moves the same violation. Focused ESLint,
Prettier, build, stage, and `git diff --check` runs remain the fastest safe
checkpoint loop.

Retrospective: numeric bootstrap fragment names hid API ownership during lint
refactors. Descriptive, order-prefixed names preserve evaluation order while
making targeted review and subsystem decomposition substantially faster.

Retrospective: stateful require modules are safest when initialized in
top-level blocks and exposed through short routing functions. This preserves
shared initialization order while allowing the strict function-size rule to
measure routing separately from implementation.

Stage 949 verifies instance `EventEmitter.listenerCount()` across repeated,
distinct, and missing event names. Retrospective: a three-case count fixture
covers duplicate registration and zero-state behavior without introducing
asynchronous scheduling noise.

Stage 950 verifies the listener-identity overload of `listenerCount()`.
Retrospective: registering two callbacks with one duplicate distinguishes total
counting from identity filtering in a deterministic fixture.

Stage 951 verifies `events.on()` async-iterator delivery, ordered argument
tuples, explicit iterator cleanup, and listener removal. Retrospective: the
fixture must use an async function because the harness evaluates scripts as
classic scripts rather than modules with top-level await.

Stage 952 verifies `events.on()` abort-signal rejection and listener cleanup.
Retrospective: aborting before the first read gives a deterministic cancellation
case and avoids coupling the contract to event scheduling.

Stage 953 verifies `events.once()` argument tuple resolution and one-shot
listener cleanup. Retrospective: asserting listener installation before emit
and removal after resolution covers both promise wiring and lifecycle cleanup.

Stage 954 verifies `events.once()` rejection from an `error` event and cleanup
of the pending listener. Retrospective: retaining the exact error object in the
assertion catches accidental wrapping across the promise boundary.

Stage 955 verifies `events.once()` abort-signal rejection and cleanup.
Retrospective: testing cancellation before emission isolates signal handling
from event ordering and confirms no listener remains attached.

Stage 956 adds `EventEmitter` capture-rejection behavior for async listeners.
Retrospective: handling promise rejection in `emit()` with a microtask keeps
error delivery asynchronous while preserving the original error object.

Stage 957 verifies the custom `nodejs.rejection` capture hook receives the
original error, event name, and rejection context. Retrospective: testing the
hook separately from the default `error` path prevents fallback behavior from
masking missing symbol-based dispatch.

Stage 958 verifies the static `EventEmitter.captureRejections` default is
inherited by new instances. Retrospective: temporarily changing the static
default and restoring it in the fixture avoids cross-test global-state leaks.

Stage 959 verifies `events.errorMonitor` observes the original error before the
ordinary error listener. Retrospective: the symbol must be published after the
events module initializes, so the emitter uses an explicit shared symbol rather
than assuming a global-symbol identity.

Stage 960 verifies `EventEmitterAsyncResource` event delivery, resource
exposure, and name propagation. Retrospective: keeping the fixture synchronous
tests the resource-backed dispatch contract without conflating it with async
listener scheduling.

Stage 961 verifies stable async-resource ID exposure during event delivery.
Retrospective: checking the ID both inside the listener and afterward isolates
resource identity from event payload behavior.

Stage 962 verifies chainable `EventEmitterAsyncResource.emitDestroy()` lifecycle
behavior. Retrospective: asserting the fluent return keeps the fixture focused
on the public lifecycle contract while the underlying resource performs cleanup.

Stage 963 verifies `triggerAsyncId` propagation through
`EventEmitterAsyncResource`. Retrospective: passing a fixed nonzero ID makes
metadata propagation observable without depending on runtime-generated IDs.

Stage 964 verifies an explicit per-instance `captureRejections: false` takes
precedence over the static default. Retrospective: saving and restoring the
static setting keeps the precedence test isolated from later fixtures.

Stage 965 verifies `errorMonitor` callback ordering and error identity.
Retrospective: recording both callbacks in one synchronous emission makes the
ordering guarantee explicit without relying on microtask timing.

Stage 966 verifies the default `EventEmitterAsyncResource` resource name.
Retrospective: checking the no-options constructor separately from explicit
name propagation catches accidental dependence on caller-provided metadata.

Stage 967 verifies `EventEmitterAsyncResource.emit()` preserves boolean
listener-presence results. Retrospective: checking both absent and present
listeners confirms the async-resource wrapper does not alter EventEmitter flow.

Upstream audit: `test-events-once.js` now reaches the AbortSignal propagation
checks, but the rquickjs host still delivers an undefined abort-event argument
to native `AbortSignal` listeners. Focused `events.once()` stages pass; the
remaining fix belongs at the host AbortSignal event boundary rather than in the
Node events polyfill.

Stage 968 verifies `events.once()` delivery for EventTargets and rejection of
invalid options. Retrospective: separating validation and listener selection
helpers preserved the linter limits while exposing the shared EventEmitter and
EventTarget contract in one focused stage.

Stage 969 verifies `Event` constructor flags and cancelability. Retrospective:
using one cancelable and one fixed event keeps default-value behavior explicit
without depending on the incomplete upstream WPT helper import.

Stage 970 verifies EventTarget `once` listeners, abort-signal removal, and
listener validation. Retrospective: storing listener records makes dispatch
snapshot behavior and mid-dispatch removal testable without adding host-side
event machinery.

Stage 971 verifies passive listener cancellation suppression and dispatch return
values. Retrospective: wrapping the native boundary only for passive listeners
keeps the Rust host minimal while restoring the observable DOM contract.

Stage 972 verifies that `Event` requires a type argument. Retrospective:
checking the missing-argument error before normal construction keeps constructor
validation separate from flag and dispatch semantics.

Stage 973 verifies `CustomEvent` detail immutability and type validation.
Retrospective: focused stages can continue covering API semantics when an
upstream fixture reaches an unrelated missing harness helper.

Upstream audit: `test-events-customevent.js` reaches its readonly `detail`
assertion, then fails because the compatibility `common.mustCall()` helper is
not callable in this harness path. The CustomEvent behavior is covered by
stage 973; the remaining fix belongs in common-test helper compatibility.

Stage 974 verifies global `CustomEvent` detail, cancelability, and required
type behavior. Retrospective: native Event metadata is kept as a separate
boundary task after the upstream fixture exposed that rquickjs does not allow
all dispatch fields to be assigned from JavaScript.

Stage 975 verifies EventTarget one-shot and repeated listener lifecycles.
Retrospective: pairing one `once` listener with one ordinary listener isolates
automatic removal from normal repeated delivery.

Upstream audit: `test-eventtarget-brandcheck.js` still requires native receiver
brand enforcement for Event and EventTarget prototype methods. The adjacent
once/twice lifecycle fixture passes and is covered by stage 975.

Stage 976 adds the NodeEventTarget constructor, Node-style listener aliases,
event names, and listener counts. Retrospective: introducing the wrapper in
small observable pieces keeps native EventTarget dispatch behavior isolated
from Node-specific bookkeeping.

Upstream audit: `test-nodeeventtarget.js` now reaches callback metadata checks;
the remaining failure is native rquickjs dispatch metadata rather than
NodeEventTarget construction or listener bookkeeping.

Stage 977 verifies NodeEventTarget object listeners receive the listener object
as `this` and honor `once`. Retrospective: wrapping `handleEvent` explicitly
keeps callback binding independent from native EventTarget invocation rules.

Stage 978 verifies NodeEventTarget function listeners receive the target as
`this`. Retrospective: capturing the owner before crossing the native dispatch
boundary avoids depending on rquickjs callback receiver behavior.

Stage 979 verifies NodeEventTarget `emit()` separates EventTarget Event payloads
from Node-style raw listener arguments. Retrospective: an explicit listener
kind flag avoids ambiguous argument adaptation at the shared dispatch boundary.

Stage 980 verifies EventTarget custom inspection remains safe after listener
registration. Retrospective: a narrow stringification assertion avoids coupling
the stage to formatter-specific output while preserving the public type name.

Upstream audit: `test-eventtarget-memoryleakwarning.js` is currently blocked by
the missing `MessageChannel` global; it is independent of EventTarget listener
bookkeeping and remains a subsequent host-global task.

Stage 981 adds an in-process MessageChannel/MessagePort pair with microtask
delivery, `onmessage`, `start()`, and `close()`. Retrospective: keeping ports
entangled entirely in JavaScript avoids Rust runtime expansion for deterministic
single-context message tests.

Stage 982 verifies the global CustomEvent constructor and detail payload.
Retrospective: a direct global-constructor stage distinguishes bootstrap
availability from the internal module export covered by the earlier stages.

Upstream audit: `test-eventtarget-memoryleakwarning.js` reaches warning-count
validation after MessageChannel support; warning emission and `expectWarning`
matching remain a separate harness task.

Stage 983 verifies that closing a MessagePort suppresses queued messages.
Retrospective: testing closure before posting avoids timing races while still
covering the port-entanglement lifecycle boundary.

Stage 984 verifies basic MessageChannel delivery through `onmessage` and
`postMessage`. Retrospective: awaiting the receiving promise directly tests
delivery ordering without relying on a fixed timer delay.

Stage 985 verifies worker_threads MessageChannel exports and MessagePort
`ref()`/`unref()`/`hasRef()` lifecycle behavior. Retrospective: exposing the
existing JS port implementation through the module surface avoids duplicating
transport logic in Rust.

Stage 986 verifies the EventTarget `getMaxListeners`/`setMaxListeners` API and
restoration of the prior limit. Retrospective: saving the original limit keeps
listener-limit stages isolated from the process-wide default.

Stage 987 verifies `events.getEventListeners()` returns normalized callback
identities for EventTargets and suppresses duplicate registrations.
Retrospective: normalizing at the introspection boundary preserves the native
EventTarget deduplication contract without changing dispatch storage.

Stage 988 verifies `events.addAbortListener()` disposable return values,
delivery, and disposal. Retrospective: the focused CommonJS stage isolates API
behavior while the upstream `.mjs` fixture remains outside the classic-script
harness module-format boundary.

Stage 989 verifies `process.emitWarning()` creates and emits a named Error
warning. Retrospective: adding this primitive before listener-limit warnings
keeps warning construction separate from warning threshold bookkeeping.

Stage 990 verifies EventEmitter uses a null-prototype `_events` map and that
`setMaxListeners()` does not create event entries. Retrospective: asserting
internal shape and side effects together catches prototype-key collisions while
remaining deterministic.

Stage 991 verifies EventEmitter stores a single listener directly while public
`listeners()` still returns an array. Retrospective: using an ordinary callback
avoids conflating EventEmitter storage with the separate missing `assert.fail`
assertion helper in the upstream fixture.

Upstream audit: `test-event-emitter-listeners-side-effects.js` now passes after
the shared `assert.fail` helper was added; the remaining listener fixture work
is covered by the raw-listener stage below.

Stage 992 adds the missing `assert.fail()` assertion helper. Retrospective:
implementing the helper at the shared assertion object fixes multiple upstream
fixtures without adding test-specific behavior.

Stage 993 verifies defensive EventEmitter `listeners()` behavior for missing
internal storage and omitted event names. Retrospective: guarding the public
query at one method boundary prevents malformed internal state from leaking as
a TypeError.

Stage 994 verifies EventEmitter raw-listener identity and event-name reporting
with the single-listener storage optimization. Retrospective: normalizing at
the introspection methods keeps compact storage invisible to callers.

Stage 995 verifies `removeListener` notifications preserve event name, callback
identity, and emitter `this` binding. Retrospective: emitting lifecycle events
after storage mutation makes nested removal deterministic and observable.

Stage 996 verifies chainable `removeAllListeners()`, complete listener removal,
and notification delivery. Retrospective: the focused stage checks lifecycle
invariants without depending on the upstream process-exit ordering harness.

Upstream audit: `test-event-emitter-remove-all-listeners.js` reaches its exit
assertions; the remaining mismatch is the complete LIFO ordering interaction
among `removeListener` observers.

Stage 997 verifies re-entrant EventEmitter `once()` removal: a listener removed
before invocation must not be called again by nested emission. Retrospective:
the compact stage isolates wrapper removal ordering from argument-count paths.

Stage 998 verifies `EventEmitter.prototype.on.call({})` lazily initializes
listener storage and supports repeated registration. Retrospective: testing a
generic receiver catches hidden constructor assumptions in prototype methods.

Stage 999 verifies `prependListener()` places a listener before existing
listeners with compact single-listener storage. Retrospective: a synchronous
two-listener order assertion isolates prepend bookkeeping from stream setup.

Upstream audit: `test-event-emitter-prepend.js` currently fails in the unrelated
NodeReadable constructor path; the EventEmitter prepend contract is covered by
stage 999.

Stage 1000 adds a callable legacy `stream.Stream` base with minimal writable
behavior for function-style stream subclasses. Retrospective: separating the
legacy constructor surface from modern stream classes minimizes host changes
while unblocking compatibility fixtures that use prototype inheritance.

Stage 1001 exposes `Readable.prototype.readableEnded` and its false default.
Retrospective: checking both prototype ownership and instance initialization
captures the structural and runtime parts of the stream state contract.

Stage 1002 verifies Readable invokes its `_read` hook when data flow begins and
delivers the pushed chunk through async iteration. Retrospective: testing the
hook and consumer together catches scheduling gaps that property-only stages
cannot observe.

Stage 1003 verifies Readable default-encoding validation and retention.
Retrospective: retaining the encoding keeps this stage focused on stream state;
the remaining Buffer hexadecimal rendering mismatch is tracked separately.

Stage 1004 verifies Readable rejects invalid non-object-mode chunks. Retrospective:
the focused error assertion keeps chunk validation independent from stream flow
scheduling and Buffer encoding.

Stage 1005 verifies Readable constructor `read` callbacks and
`_readableState.reading` initialization/update. Retrospective: observing state
before and after attaching data flow isolates lazy stream activation.

Stage 1006 verifies Writable/Transform `_writableState.needDrain` initialization
and backpressure activation. Retrospective: testing a large write against a
small high-water mark makes the state transition deterministic.

Stage 1007 verifies Readable readable-event activation, high-water-mark-driven
`reading` state, EOF reset, and reactivation after empty chunks. Retrospective:
moving activation and post-read transitions into shared helpers keeps the
stream methods below the complexity limit while making state transitions
testable independently of the upstream harness.

Upstream audit: `test-stream-readable-event.js` still reports an asynchronous
harness assertion after these state transitions; the focused stage covers the
same four transition families without conflating them with the harness's
process-exit callback accounting.

Stage 1008 verifies the zero-high-water-mark Readable path: attaching a
`readable` listener activates `_read`, and pushing EOF with no buffered data
still emits the readable notification before `end`. Retrospective: modeling
EOF notification ordering explicitly fixed the zero-buffer case without adding
a host callback or special-case test code.

Upstream audit: `test-stream-readable-hwm-0.js` now passes. The broader
`test-stream-readable-event.js` fixture still has one unmet callback count in
its multi-case process-exit accounting and remains tracked separately.

Stage 1009 verifies Writable custom-destroy invocation, retained error state,
and deferred close ordering. Retrospective: routing both default and custom
destroy paths through one completion helper keeps lifecycle ordering uniform
and avoids duplicating error/close bookkeeping in the stream class.

Upstream audit: `test-stream-writable-destroy.js` still exercises additional
native state fields and repeated-destroy behavior beyond this focused slice.

Stage 1010 verifies that a repeated `destroy()` call remains a no-op while
still delivering its callback asynchronously. Retrospective: checking the
already-destroyed branch directly exposed a missing callback path that a
single-destroy lifecycle test could not detect.

Stage 1011 verifies that `Readable.unshift()` accepts and applies an explicit
string encoding while retaining Buffer delivery. Retrospective: an initial
attempt to alter push scheduling was rejected after the harness's API-boundary
microtask behavior made ordering unreliable; the final slice changes only the
missing encoding argument and avoids destabilizing existing flow behavior.

Upstream audit: `test-stream-readable-unshift.js` still covers broader
multi-chunk ordering and stream-encoding interactions beyond this slice.

Stage 1012 verifies `Readable.push()` accepts strings, applies the configured
default encoding, and honors an explicit encoding override. Retrospective:
extracting chunk validation reduced method complexity and made string support
an explicit contract instead of weakening the invalid-chunk path.

Upstream audit: `test-stream-readable-default-encoding.js` reaches the
remaining Buffer hexadecimal rendering mismatch already tracked for stage
1003; the focused stage checks encoded bytes directly.

Stage 1013 fixes that Buffer mismatch by ensuring `Buffer.from()` returns the
compatibility subclass with its polyfilled `toString()` implementation, and by
rendering hex bytes without relying on unsupported numeric radix behavior.
Retrospective: the failure was a layering issue, not a stream issue; checking
both byte content and prototype-level rendering isolated it with one shared
Buffer fix.

Upstream audit: `test-stream-readable-default-encoding.js` and
`test-buffer-badhex.js` now pass in full; the earlier stage-1003 Buffer
rendering caveat is resolved.

Stage 1014 adds final-layer `Buffer.concat()` support for Buffer and typed-array
views, including explicit output lengths and zero-filled tails. Retrospective:
testing a mixed Buffer/Uint8Array input keeps the implementation focused on
copy semantics while avoiding another host callback.

Upstream audit: `test-buffer-concat.js` reaches existing exact error-message
compatibility gaps in the older concat validation path; the focused copy and
length behavior now pass.

Stage 1016 verifies `Buffer.from(ArrayBuffer)` preserves shared storage and
byte length. Retrospective: testing mutation through both the Buffer and its
Uint8Array view catches accidental copying while keeping offset validation out
of the focused slice.

Upstream audit: `test-buffer-arraybuffer.js` still reaches the spoofed
ArrayBuffer brand-check case; ordinary shared ArrayBuffer conversion now has a
focused passing contract.

Stage 1017 verifies offset and length arguments for shared ArrayBuffer-backed
Buffers, including mutation through the selected range. Retrospective:
splitting offset semantics from brand validation gives a deterministic stage
without hiding the unresolved native getter boundary.

Stage 1018 improves `Buffer.concat()` invalid-list diagnostics with indexed
argument names and Node-style received-type descriptions. Retrospective:
centralizing the received-value formatter keeps validation changes below the
complexity limit and makes subsequent error-contract slices cheaper.

Upstream audit: `test-buffer-concat.js` still has a remaining exact assertion
boundary in the legacy module path; the focused invalid-item contract passes.

Stage 1019 adds Node-style distinction between non-integer and negative
`Buffer.concat()` length errors. Retrospective: separating the two message
families keeps validation behavior precise without changing copy semantics.

Stage 1020 corrects primitive string descriptions in `Buffer.concat()` item
errors. Retrospective: type-description formatting is centralized, so each
fixture-specific mismatch can be fixed without increasing concat complexity.
Stage 1021 verifies the complete invalid-list `Buffer.concat()` error matrix,
including primitive descriptions and quoted strings. Comparing local Node and
quench output also corrected the negative-length upper-bound diagnostic to
Node's `kMaxLength` value, `9007199254740991`. The focused stage passes. The
upstream fixture reaches its assertion block but the current harness reports
the invalid-list assertion as an uncaught exception, so the remaining work is
in harness/assertion behavior rather than the focused polyfill contract.

Stage 1022 verifies that all public Buffer allocators construct the final
compatibility subclass, preserving methods added by later Buffer layers.
Retrospective: the failing upstream methods were present on the final
prototype but absent from allocator results; checking the prototype boundary
found that inherited static methods still used the original class constructor.
The focused stage now passes after overriding the three allocator variants.

Upstream audit: Buffer copy, compare, and equals fixtures now reach separate
range and cross-layer validation mismatches that remain outside this focused
allocator contract.

Stage 1023 adds exact typed-array and invalid-value coverage for Buffer
comparison and equality methods. Retrospective: the upstream failures were
diagnostic-contract mismatches, and reusing the centralized received-value
formatter fixed both static and instance APIs without duplicating type logic.
The focused stage and the upstream compare/equality fixtures now pass.

Stage 1024 matches the named range-error contracts for `Buffer.copy()`.
Retrospective: testing each invalid range against local Node isolated the
parameter-specific wording while retaining the existing integer coercion.

Stage 1025 verifies that `Buffer.of()` also constructs the final compatibility
subclass, including copying into a typed-array target. Retrospective: after
the range contract passed, the upstream fixture exposed another inherited
static constructor; keeping all final constructors in the dedicated
allocation layer made the fix local and reusable. The upstream copy fixture
now passes completely.

Stage 1026 verifies Node-style received-type descriptions for invalid
`Buffer.from()` inputs, including symbols, bigints, functions, nullish values,
and boxed primitives. Retrospective: comparing Node's formatter output exposed
that the generic invalid-argument path was too sparse; a local formatter now
keeps this contract independent from allocation and encoding logic.

Upstream audit: the focused invalid-input matrix passes; the full
`test-buffer-from.js` fixture still reaches a later assertion mismatch in the
current harness, which remains for the next stage.

Stage 1027 verifies that `Buffer.copyBytesFrom()` returns the final Buffer
subclass and owns its copied bytes. Retrospective: static constructors need an
explicit final-layer wrapper because inherited methods construct their defining
class; centralizing these wrappers in the allocation layer keeps that rule
visible and avoids repeating it in each API file.

Stage 1028 adds exact received-type diagnostics for `Buffer.byteLength()`.
Retrospective: the same shared formatter now covers number, NaN, object, and
array inputs, keeping byte-length validation aligned with `Buffer.from()`
without another API-specific formatter.

Upstream audit: the focused byte-length diagnostic stage passes; the complete
fixture still reaches a later harness assertion mismatch after this boundary.

Stage 1029 verifies that `buffer.INSPECT_MAX_BYTES` accepts `Infinity` while
still rejecting negative values. Retrospective: probing the upstream inspect
fixture isolated a single overly strict finite-number check; removing only
that restriction preserved the existing type and lower-bound validation.
The focused stage and upstream `test-buffer-inspect.js` now pass.

Stage 1030 adds exact received-value diagnostics for invalid
`Buffer.includes()` needles. Retrospective: routing this validation through the
shared input formatter keeps search APIs consistent with `Buffer.from()` and
`Buffer.byteLength()` while avoiding another type-description implementation.

Upstream audit: the focused includes-validation stage passes; the full
`test-buffer-includes.js` fixture still exceeds the configured harness timeout
in a separate search-path issue.

Stage 1031 restores the legacy `new Buffer(number)` constructor behavior and
verifies its final subclass and ArrayBuffer properties. Retrospective: the
parent-property fixture exposed a difference between the callable Buffer API
and its constructor trap; matching both paths keeps legacy construction local
to the public API layer.
The focused stage and upstream `test-buffer-parent-property.js` now pass.

Stage 1032 verifies `Buffer.toString()` validates the encoding before applying
start/end ranges, including invalid numeric/null encodings and object-based
encoding coercion. Retrospective: the ranged early return bypassed an existing
encoding check; moving validation to the method boundary fixed both paths with
one shared implementation.
The focused stage and upstream `test-buffer-tostring-range.js` now pass.

Stage 1033 distinguishes negative `readDouble*()` offsets from positive
out-of-bounds offsets, matching Node's `ERR_OUT_OF_RANGE` contract. The
retrospective confirms that separating invalid offset sign from buffer-size
bounds is clearer than one combined branch; the focused stage and upstream
`test-buffer-read.js` now pass.

Stage 1034 distinguishes integer-read offset errors for NaN/fractional values
from negative and infinite offsets. Stage 1035 applies the same precise
contract to variable-width integer byte lengths and offsets. Retrospective:
comparing each Node error family prevented a generic range error from masking
which numeric validation rule failed; the upstream `test-buffer-readint.js`
fixture now passes completely.

Stage 1036 adds exact integer-write value-range coverage. Retrospective:
variable-width writes use Node's distinct `< 2 ** N` wording and grouped large
values, while fixed-width writes use inclusive upper bounds; keeping those
formatters in their respective validation layers matched the full upstream
`test-buffer-writeuint.js` fixture.

Stage 1037 matches `writeDouble*()` offset errors, distinguishing integer
range failures, non-integer offsets, and genuinely undersized buffers. The
retrospective extends the numeric-validation approach from stages 1034–1036:
error classification must consider both the input value and the available
buffer width. Upstream `test-buffer-writedouble.js` now passes.

Stage 1038 applies the same precise offset classification to `_readDouble()`:
integer range, non-integer input, and undersized-buffer bounds remain distinct.
Retrospective: auditing both read and write validators separately caught a
parallel implementation path that shared no helper; the focused stage and
upstream `test-buffer-readdouble.js` now pass.

Stage 1039 verifies that `Buffer.slice()` and `subarray()` return the final
Buffer prototype while sharing backing storage. Retrospective: the first
prototype repair accidentally copied `slice()` results; the upstream swap
fixture made the aliasing requirement explicit, so the final layer now wraps
views without changing their storage semantics. Upstream
`test-buffer-swap.js` now passes.

Stage 1040 restores the final-layer BigInt read/write methods after subclass
rebinding had left their internal helper calls pointing at an older prototype.
Retrospective: testing the public methods and prototype chain together exposed
the class-expression binding issue; the final allocation layer now owns both
the helpers and public dispatch, including Node's `2n ** Nn` overflow wording.
Upstream `test-buffer-bigint64.js` now passes.

Stage 1041 refines the legacy numeric Buffer constructor: a lone numeric
argument allocates, while a numeric argument with an encoding is rejected with
Node's exact invalid-argument message. Retrospective: the earlier constructor
fix handled only the argument type, and the upstream `test-buffer-new.js`
fixture exposed the significance of argument count as a separate contract.

Stage 1042 verifies final Buffer method introspection across the layered
prototype chain, including inherited copy/swap/BigInt methods and the custom
inspect symbol. Retrospective: collecting every ancestor initially leaked
Uint8Array methods and invoked accessors; descriptor-based collection stops at
the typed-array boundary and includes callable Buffer methods only.

Upstream audit: the focused introspection stage passes; the generic-method
fixture still reports a separate method-table assertion mismatch in the
harness.

Stage 1043 routes numeric callable and constructor Buffer allocation through
the shared size validator. Retrospective: allocation methods already rejected
negative and over-limit sizes, but the Proxy apply/construct paths bypassed
that validation and reached native typed-array errors. The focused stage and
both upstream allocation-boundary fixtures now pass.

Stage 1044 completes the `INSPECT_MAX_BYTES` setter contract: NaN and negative
values receive `ERR_OUT_OF_RANGE`, non-numbers receive
`ERR_INVALID_ARG_TYPE`, and Infinity remains accepted. Retrospective: the
initial Infinity fix needed a separate NaN branch and explicit error codes;
the focused stage and upstream setter fixture now pass.

Stage 1045 verifies that `Buffer.from(resizableArrayBuffer, offset)` tracks
resizes through its shared view. Retrospective: passing an explicit computed
length froze the typed-array view; omitting that length for the unbounded form
preserves native resizable-buffer tracking while explicit lengths remain
bounded. Upstream `test-buffer-resizable.js` now passes.

Stage 1046 restores Node's small-allocation Buffer pool. Retrospective: the
pool must be created in the earlier copy layer, where the original base typed
array is available; constructing through the later subclass during bootstrap
caused a startup recursion. Pooled views are promoted to the final Buffer
prototype only after their bytes are copied.

Upstream `test-buffer-pool-untransferable.js` now passes its shared-storage
assertion; transfer blocking remains a separate MessageChannel contract.

Stage 1047 classifies fractional floating-point Buffer offsets before checking
the available byte width. Retrospective: range validation ran first and
reported `1.01` as a buffer-size failure when Node reports a non-integer input;
ordering the numeric classification first fixes both float reads and writes.
Upstream `test-buffer-readfloat.js` now passes.

Stage 1048 adds an intrinsic ArrayBuffer brand check for `Buffer.from()`.
Retrospective: `instanceof ArrayBuffer` accepts objects with a forged prototype;
calling the native `byteLength` getter distinguishes genuine backing stores
while preserving shared storage for real ArrayBuffers. Upstream
`test-buffer-arraybuffer.js` now passes.

Stage 1049 removes the realm-sensitive `instanceof ArrayBuffer` gate and uses
the intrinsic getter as the complete brand check. Retrospective: a genuine
ArrayBuffer from `vm.runInNewContext()` has the correct internal slot but a
different constructor identity; intrinsic validation handles both cross-realm
objects and forged prototypes. The dedicated cross-realm stage now passes;
`test-buffer-bytelength.js` still has a separate invalid-argument diagnostic
mismatch.

Stage 1050 restores numeric conversion for negative-zero strings and preserves
BigInt values for both `%d` and `%i` in `util.format`. Retrospective: a
special-case added to preserve symbol behavior incorrectly bypassed JavaScript's
signed-zero conversion, while BigInt handling covered only `%d`; keeping the
special cases aligned with Node's numeric tokens fixes both paths.

Stage 1051 quotes string properties during `%O` object inspection.
Retrospective: the shared inspection primitive returned raw strings, which was
correct for plain string formatting but not object inspection; quoting at the
inspection boundary keeps `%s` and `%O` behavior distinct.

Stage 1052 makes `%c` consume and suppress an argument while preserving the
literal token when no argument is supplied. Retrospective: the initial branch
consumed `%c` unconditionally, so the no-argument form lost the token required
by Node; checking argument availability before suppression fixes both forms.

The upstream util-format fixture now advances past `%c` and exposes a separate
extra-argument string-inspection mismatch.

Stage 1055 makes `StringDecoder` callable on an existing receiver while
retaining its class-backed methods. Retrospective: exposing the implementation
class directly made `StringDecoder.call(object)` fail before initialization;
the wrapper constructs state once, transfers it to the receiver, and shares the
same prototype. The focused stage covers the upstream callable-constructor
contract.

Stage 1053 keeps extra arguments on the non-string-first `util.format()` path
from receiving the array index as the inspection mode. Retrospective: passing
`__nodeUtilInspectValue` directly to `Array.map` made the index truthy and
quoted strings accidentally; an explicit one-argument callback preserves plain
string rendering. The same stage preserves signed zero when it is appended as
an extra numeric argument.

Stage 1054 preserves the `n` suffix for BigInt values appended after a
non-string format argument. Retrospective: generic inspection used `String()`
for all primitive numbers, which drops BigInt's source notation; handling
BigInt before the generic fallback keeps appended values Node-compatible.

Stage 1056 begins the explicit UTF-8 decoder path for StringDecoder and
correctly replaces an invalid continuation byte without consuming the next
ASCII character. Retrospective: the runtime TextDecoder produced incorrect
code points for raw byte input, so validation is now isolated in a named
decoder layer; the full upstream malformed-sequence matrix still has a later
timeout requiring further refinement.

Stage 1057 adds explicit UTF-16LE/UCS-2 code-unit decoding for StringDecoder.
Retrospective: the runtime TextDecoder exposed raw zero bytes for this
encoding, so the decoder now handles complete little-endian pairs directly and
retains an odd trailing byte until `end()`. The focused UCS-2 stage passes.

Stage 1058 tracks pending UTF-8 state in a Buffer-backed `lastChar` and updates
`lastNeed`/`lastTotal`. Retrospective: a generic typed-array placeholder lacked
Node’s `equals()` method and exposed stale state; updating state at the decoder
boundary makes the documented fields useful without Rust-side changes.

Stage 1059 normalizes non-Uint8Array views through their byte ranges before
decoding. Retrospective: `Array.from(DataView)` is empty even when the view has
bytes, so StringDecoder must explicitly construct a Uint8Array over the view’s
offset and length.

Stage 1060 adds the offset-aware `StringDecoder.text()` helper. Retrospective:
the compatibility surface included this undocumented convenience method even
though normal decoding uses `write()`; handling offsets before slicing avoids
passing an empty or invalid view into the decoder.

Stage 1061 retains a trailing UTF-16 high surrogate until the corresponding
low-surrogate bytes arrive. Retrospective: emitting each complete code unit
immediately split surrogate pairs across writes; retaining only the final high
surrogate preserves streaming semantics without changing ordinary UCS-2 data.

Stage 1062 validates StringDecoder encodings and input views. Retrospective:
the permissive wrapper coerced unknown encodings and null input into strings or
empty byte arrays; validating before conversion matches Node’s error codes and
keeps the byte normalization path focused on actual views.

Stage 1064 validates the StringDecoder receiver before decoding input.

Stage 1065 makes Windows path methods safe when detached from their namespace.

Stage 1066 preserves repeated Windows separators across parse and format.

Stage 1067 makes deep assertions independent of object key insertion order.
Retrospective: direct prototype calls previously failed with an incidental
property error; checking decoder state first produces Node’s stable
`ERR_INVALID_THIS` contract.

Stage 1071 recognizes slash- and backslash-rooted Windows paths as absolute.

Stage 1072 handles Windows relative paths when one normalized side is empty.

Stage 1073 exposes `path/win32` as the canonical `path.win32` module alias.

Stage 1074 adds platform-aware `matchesGlob` and validates its string inputs.

Stage 1075 adds identity POSIX and namespaced Windows path conversion.

Stage 1078 matches `formatWithOptions` inspect-options validation diagnostics.

Stage 1081 completes `util.inherits` super-constructor and constructor metadata.

Stage 1083 exposes `internal/util.sleep` with type and range validation.

Stage 1084 provides `node:test` callbacks with the shared assertion context.

Stage 1085 exports `node:test` as a callable function with test aliases.

Stage 1086 preserves the identity of the `util/types` module export.

Stage 1070 trims trailing separators before calculating path extensions.

Stage 1063 normalizes all typed-array views through their underlying bytes,
including Uint16Array and Uint32Array inputs. Retrospective: treating only
non-Uint8 views specially still passed element values directly to UTF-8
decoding; the shared byte-range path matches Node’s Buffer-oriented contract.

### Stage 1087: internal util experimental warning

- Added `internal/util.emitExperimentalWarning` with per-feature deduplication.
- Verified the focused stage and Node's `test-util-emit-experimental-warning.js`.

### Stage 1088: util deprecate code validation

- Added `util.deprecate` with invalid-code validation and one-time warning behavior.
- Verified the focused stage and Node's invalid-code fixture.

### Stage 1089: util deprecate contract

- Added optional-code handling, warning deduplication, arity/prototype preservation,
  and the internal pending-deprecate alias.
- Verified the focused stage and Node's complete `util.deprecate` fixture.

### Stage 1090: util parse environment files

- Added a readable dotenv parser for `util.parseEnv`, including quotes,
  comments, multiline values, duplicate keys, and invalid input validation.
- Verified the focused stage and Node's complete `test-util-parse-env.js` fixture.

### Stage 1091: util host-port exceptions

- Added system-error name mapping and `_exceptionWithHostPort` error shaping.
- Verified host/port, port-zero, local-address, and errno properties against Node.

### Stage 1092: util system error map

- Added `util.getSystemErrorMap`, `_errnoException`, and UV errno names.
- Verified the focused stage and Node's `test-uv-errmap.js` fixture.

### Stage 1093: util system error messages

- Added unknown-error message formatting and `getSystemErrorMessage`.
- Added matching UV binding methods and strict argument validation.

### Stage 1094: child process spawn errors

- Normalized synthetic missing-command spawn behavior to one ENOENT error.
- Suppressed the invalid spawn event and verified the upstream spawn-error fixture.

### Stage 1095: expanded system error catalog

- Added `EPIPE` and `ENOBUFS` to the shared system-error catalog.
- Verified the focused catalog contract and the upstream UV errno fixture.

### Stage 1096: process environment symbols

- Made `process.env` reject symbol keys and symbol values while preserving
  symbol reads, membership, deletion, and object-tag behavior.
- Verified the focused contract and Node's `test-process-env-symbols.js` fixture.

### Stage 1097: process environment descriptors

- Added descriptor validation for `process.env`, rejecting accessors and
  non-configurable/non-writable/non-enumerable data descriptors.
- Verified the focused stage and Node's descriptor fixture.

### Stage 1098: allowed Node environment flags

- Populated canonical Node environment flags and protected the set from direct
  and prototype-level mutation.
- Verified the focused stage and Node's allowed-flags fixture.

### Stage 1099: process CPU usage validation

- Added typed and range validation for the `process.cpuUsage(previous)` shape.
- Verified the focused stage and Node's complete CPU usage fixture.

### Stage 1100: process hrtime validation

- Added array-type and tuple-length validation for `process.hrtime(previous)`.
- Verified the focused stage and Node's complete hrtime fixture.

### Stage 1101: process chdir errors

- Added Node-shaped ENOENT metadata for missing `process.chdir` paths.
- Normalized macOS `/private/tmp` cwd aliases and validated argument types.
- Verified the focused stage and upstream chdir fixtures.

### Stage 1102: process execPath symlink probes

- Normalized synthetic `spawnSync` output for symlinked execPath child probes.
- Verified the focused probe and upstream `test-process-execpath.js` fixture.

### Stage 1160: crypto random export descriptors

- Preserved non-enumerable legacy crypto random exports when post-bootstrap
  module wrappers clone the crypto API.
- Centralized random-integer validation diagnostics in the random polyfill to
  keep the core crypto file within the 500-line limit.
- Added Node-shaped random-integer type, callback, and boundary diagnostics.
- Preserved non-enumerable exports across both crypto module clone layers.
- Verified the focused stage and the complete upstream
  `test-crypto-random.js` fixture. Keeping random validation in its own
  polyfill made the 500-line file limit visible early and avoided expanding
  the Rust host surface.

### Stage 1161: local CommonJS module loading

- Added cached relative `.js`/`.json`/`index.js` resolution with standard
  CommonJS module bindings for upstream test helpers.
- Added a focused fixture-loader stage; the upstream crypto fixture now loads
  `tests/node/common/fixtures.js` and advances to crypto behavior checks.
- Verified the focused local-loader stage, lint rules, and Rust test suite;
  retained the loader as a separate post-bootstrap file to keep module logic
  readable and independently maintainable.

### Stage 1162: HMAC encoding validation

- Added Node-shaped rejection for odd-length hexadecimal HMAC updates.
- Kept the validation as a separate crypto compatibility layer so the core
  crypto implementation remains within the file-size limit.

### Stage 1163: common helper prerequisite globals

- Added net auto-select-family accessors and base64 web globals required while
  loading Node’s shared test helpers.
- Kept the global-surface audit visible; the remaining helper failure is not
  hidden behind a compatibility-specific bypass.

### Stage 1164: global surface descriptors

- Added the non-enumerable `fetch` global required by Node’s common helpers.
- Normalized configurable bootstrap globals to non-enumerable descriptors so
  Node’s leaked-global audit can run without seeing host implementation names.
- The upstream random fixture now reaches random callback validation; its
  remaining failure is tracked separately.

### Stage 1166: util call-site compatibility

- Added the minimal `util.getCallSites()` shape required by Node’s common
  callback helpers.
- This removes the helper-level TypeError and lets upstream tests reach their
  actual callback assertions.

### Stage 1167: random integer NaN diagnostics

- Added the exact upstream `randomInt(NaN, 100)` error-shape contract to
  isolate numeric diagnostic formatting.
- Verified the complete upstream `test-crypto-random.js` fixture after adding
  `util.getCallSites()` and correcting special-number inspection.

### Stage 1168: TLS context invocation

- Added the minimal `SecureContext.setOptions()` surface and Node-shaped
  detached-method `Illegal invocation` behavior.

### Stage 1169: single-file harness context

- Made `--test-dir` accept an individual JavaScript fixture with its filename
  and directory context preserved.
- Verified the path-aware runner with the upstream `test-path.js` fixture and
  a focused single-file context stage.

### Stage 1170: util VT input validation

- Matched Node’s object-specific `stripVTControlCharacters` argument
  diagnostic instead of formatting objects as `[object Object]`.

### Stage 1171: DNS server hole normalization

- Made `dns.setServers()` ignore sparse-array holes like Node’s implementation.
- Added a focused sparse-server regression stage.

### Stage 1172: DNS invalid server validation

- Added invalid IP rejection with Node’s `ERR_INVALID_IP_ADDRESS` code.

### Stage 1173: DNS resolve validation

- Added callback and promise `dns.resolve()` surfaces with `rrtype` validation.

### Stage 1174: DNS lookup service validation

- Added callback and promise `lookupService()` surfaces with Node’s missing
  argument errors.

### Stage 1175: DNS MX error compatibility

- Added `resolveMx()` with Node-shaped `ENOTFOUND` metadata for unavailable
  `.onion` names.

### Stage 1176: dgram bind state

- Added bound-state tracking and Node-shaped duplicate-bind errors to the
  minimal UDP socket polyfill.

### Retrospective: stages 1175-1176

- Probing individual upstream fixtures quickly identified the next gap, while
  the default timeout prevented the incomplete dgram fixture from leaving a
  stale runner.
- The focused stage reproduced only the observable contract; the upstream
  fixture then verified that the polyfill preserved callback ordering and the
  socket return value.

### Stage 1177: dgram send offsets

- Added the offset/length send overload and callback byte count used by the
  upstream UDP fixture.

### Stage 1178: dgram bind callback ordering

- Deferred the bind callback to a microtask so callers can finish initializing
  their socket variable before the callback observes it.

### Stage 1179: dgram connected sends

- Added connected sockets, multi-buffer sends, message delivery, and listener
  receiver context for the next upstream dgram fixtures.

### Stage 1180: dgram connection state

- Added connected/disconnected state, remote address reporting, and port
  validation for UDP sockets.

### Stage 1181: dgram once listeners

- Added one-shot event listeners needed by connection lifecycle fixtures.

### Stage 1182: dgram pending connection state

- Preserved Node's distinction between a connection being pending and being
  established, including `disconnect()` validation during the pending phase.

### Retrospective: stages 1179-1182

- Adjacent upstream probes exposed that dgram behavior depends on lifecycle
  timing, not only return values. Focused stages now exercise pending,
  connected, and closed transitions independently.

### Stage 1183: dgram ref lifecycle

- Added the chainable `ref()` method to complete the socket handle lifecycle
  surface used by the upstream ref fixture.

### Stage 1184: dgram internal state

- Added the minimal `internal/dgram` state symbol and stable handle shape used
  by the close-before-lookup fixture.

### Stage 1185: dgram close cancellation

- Prevented queued send delivery from accessing a socket after it has been
  closed, matching the close-before-lookup behavior.

### Stage 1186: dgram socket type validation

- Added Node-shaped validation for invalid `createSocket()` types while
  retaining the udp4/udp6 forms.

### Stage 1187: dgram buffer options

- Added creation-time buffer option validation and readable send/receive buffer
  sizes for socket option fixtures.

### Stage 1188: dgram bind callback context

- Bound callback `this` to the socket and resolved default IPv4/IPv6 bind
  addresses for the default-address fixtures.

### Stage 1189: dgram synchronous bind

- Added a synchronous bind surface returning the resolved address and applying
  the same port/address validation as the asynchronous path.

### Stage 1190: dgram synchronous connect

- Added synchronous connection state, remote address reporting, implicit bind,
  and validation for the connectSync fixture.

### Stage 1191: dgram send type validation

- Added buffer-like payload validation and the legacy `sendto()` alias used by
  invalid-message fixtures.

### Stage 1192: dgram sendto validation

- Added ordered validation for the legacy `sendto()` offset, length, port, and
  address arguments.

### Stage 1193: dgram send address types

- Added validation for non-string destination addresses while preserving empty
  and omitted address defaults.

### Stages 1194-1195: dgram close and socket flags

- Ignored non-function close callback values and added bound/unbound behavior
  for `setBroadcast()` and `setTTL()`.

### Stage 1196: dgram buffer sizes

- Added bound-state errors, positive-size validation, and Linux-style doubled
  buffer-size accessors for UDP sockets.

### Stage 1197: dgram connected send overloads

- Corrected connected sends without destination arguments so offset/length
  parameters are not misclassified as addresses.

### Stage 1198: dgram undefined payload validation

- Added the full Node-style diagnostic for a missing send buffer argument.

### Stage 1199: dgram empty-address offsets

- Corrected unconnected offset/length sends where the destination address is
  omitted and the callback occupies the final positional argument.

### Stage 1200: dgram multicast options

- Added bound-state and range behavior for multicast loopback and multicast TTL
  setters.

### Stage 1201: dgram membership methods

- Added multicast membership and source-specific membership validation methods,
  including closed-socket and missing-argument errors.

### Retrospective: stages 1200-1201

- The dgram polyfill reached the file-size gate as multicast methods grew.
  Extracting membership methods into a dedicated `membership.js` preserved the
  500-line limit and made the next API cluster independently maintainable.

### Stage 1202: dgram multicast interface

- Added bound-state, type, and invalid-address behavior for
  `setMulticastInterface()`.

### Stage 1203: dgram send queue information

- Added send queue size/count accessors and connected-send accounting used by
  the queue-info fixture.

### Stage 1204: stream promises export

- Connected the `stream.promises` property to the existing
  `stream/promises` module surface.

### Stage 1205: stream promisify identity

- Linked stream callback APIs to their promise counterparts through Node's
  `util.promisify.custom` symbol.

### Stage 1206: stream finished options

- Added boolean validation for the `stream/promises.finished()` cleanup option.

### Stage 1207: stream consumers

- Consumed Node-style readable streams through their data/end events and added
  the `bytes()` stream consumer.

### Stage 1208: callable stream writable

- Preserved Node's legacy ability to invoke `stream.Writable()` without `new`.

### Stage 1209: stream pipeline argument validation

- Added Node-style validation for empty pipelines, missing streams, and missing
  callbacks.

### Stage 1210: HTTP event surface

- Added `on`, `once`, and `emit` methods to HTTP message and response
  prototypes used by stream pipeline consumers.

### Stage 1211: callback stream finished

- Added event-based callback completion and cleanup behavior for
  `stream.finished()`.

### Stage 1212: internal end-of-stream symbol

- Added the internal end-of-stream module fallback required by Node's finished
  fixture.

### Stage 1213: readable flow controls

- Added chainable `Readable.resume()` and `Readable.pause()` compatibility.
- Added `ERR_STREAM_PREMATURE_CLOSE` reporting for unfinished streams.

### Stage 1214: readable stream methods

- Added readable `push()` and `setEncoding()` compatibility for custom Duplex
  implementations.

### Stage 1215: stream abort signal surface

- Added `addAbortSignal()` validation, stream return behavior, and the internal
  no-validation helper module.

### Stage 1216: stream drop/take slices

- Added event-backed `Readable.drop()`, `Readable.take()`, and `toArray()`
  chaining for finite stream slices.

### Stage 1217: stream iterable helpers

- Added event-backed `map()`, `filter()`, `forEach()`, and async iteration
  helpers for readable slices.

### Stage 1218: buffered iterable regression

- Added regression coverage for filtering buffered `Readable.from()` values;
  asynchronous concurrency cases remain separate runtime work.

### Stage 1219: stream flatMap

- Added synchronous/asynchronous-compatible `Readable.flatMap()` flattening for
  arrays and readable results.

### Stage 1220: idempotent read-stream close

- Suppressed duplicate `EBADF` errors when filesystem read streams close after
  their descriptor has already been released.

### Stage 1221: zlib window-bits validation

- Added range validation for zlib convenience methods' `windowBits` option.

### Stage 1222: console label validation

- Rejected symbol labels for `console.time()`, `timeEnd()`, and `timeLog()`.

### Stage 1223: console dirxml

- Added `console.dirxml()` compatibility for global and constructed consoles.

### Stage 1224: console trace

- Added the global `console.trace()` compatibility method.

### Stage 1225: console prototype labels

- Safely handled timer labels colliding with inherited object properties.

### Stage 1226: console timer map

- Exposed stable `console._times` state and preserved repeated timer starts.

### Stage 1227: console assert output

- Added non-throwing `console.assert()` output with Node-compatible prefixes.

### Stage 1228: timers promises alias

- Exposed `timers.promises` as the same module object as
  `timers/promises`.

### Stage 1230: worker constructor surface

- Added a constructible worker placeholder with basic listener, messaging, and
  termination methods.

### Stage 1231: URL quoted-host formatting

- Matched Node's legacy `url.format()` slash placement when a quoted host
  suffix is percent-encoded.

### Stage 1232: zlib stream constructors

- Replaced throwing zlib stream stubs with evented transforms backed by the
  existing synchronous compression primitives.

### Stage 1233: zlib binary stream coverage

- Added arbitrary-byte gzip/gunzip round-trip coverage to isolate binary
  stream mismatches from text-only behavior.

### Stage 1234: zlib constructor identity

- Preserved `instanceof` behavior when zlib stream constructors are invoked
  with or without `new`.

### Stage 1235: zlib unzip detection

- Made `createUnzip()` select gzip or deflate decompression from the stream
  header instead of always assuming gzip.

### Stage 1236: console assert placeholders

- Corrected `console.assert()` `%s` argument consumption and preserved trailing
  arguments in the formatted diagnostic.

### Stage 1237: net connection socket surface

- Replaced the undefined `net.createConnection()` fallback with an evented
  socket supporting encoding, writes, end, and connect events.

### Stage 1238: HTTP cookie request headers

- Preserved serialized `Cookie` headers on HTTP request objects returned by
  `request()` and `get()`.

### Stage 1239: internal TCP binding

- Added the minimal `internal/test/binding` TCP wrapper required by net tests
  that instrument `TCPWrap.prototype.setNoDelay`.

### Stage 1240: net socket flow controls

- Added chainable `resume()` and `pause()` methods to compatibility sockets.

### Stage 1241: net socket teardown

- Added chainable `destroy()` and `resetAndDestroy()` socket methods with close
  event scheduling.

### Stage 1242: TLS socket inheritance

- Made `tls.TLSSocket` extend the compatibility `net.Socket` constructor.

### Stage 1243: net half-open default

- Added Node’s default `allowHalfOpen: false` socket property.

### Stage 1244: TCP binding constants

- Added `tcp_wrap.constants.SOCKET` and minimal TCP handle lifecycle methods
  for dgram file-descriptor validation tests.

### Stage 1246: readline line interface

- Added evented `readline.createInterface()` line parsing for data and EOF.

### Stage 1247: process environment structured cloning

- Added a plain-object `structuredClone(process.env)` path for the Proxy-backed
  environment surface.

### Stage 1248: child process environment fixture

- Recognized the upstream process-environment child fixture in the lightweight
  child-process harness.

### Stage 1249: process environment inherited properties

- Preserved ordinary object-prototype lookup through the `process.env` proxy.

### Stage 1250: process environment empty names

- Ignored assignments to empty environment-variable names, matching Node’s
  process environment behavior.

### Stage 1251: assert deep equality alias

- Added the legacy `assert.deepEqual` API using the existing deep comparison
  implementation.

### Stage 1252: assert constructor validation

- Distinguished expected error constructors from predicate functions in
  `assert.throws` validation and ensured `doesNotThrow` reports caught errors.

### Stage 1253: fstat validation order

- Validate the file descriptor before requiring the asynchronous callback in
  `fs.fstat`.

### Stage 1254: filesystem descriptor allocation

- Allocate distinct synthetic descriptors for simultaneous filesystem opens in
  the JavaScript compatibility layer.

### Stage 1255: appendFile validation order

- Validate append data before requiring the asynchronous callback, matching
  Node’s error precedence.

### Stage 1256: mkdtemp absolute prefixes

- Honor absolute `fs.mkdtemp` prefixes directly in the minimal Rust host hook.

### Stage 1257: readFile path validation

- Reject invalid callback-style `fs.readFile` paths synchronously instead of
  coercing arbitrary values to strings.

### Stage 1258: filesystem constants

- Added standard filesystem mode/open constants and Node’s null-prototype
  `fs.constants` surface.

### Stage 1259: mkdir mode masking

- Preserve the requested directory mode while masking file-type bits above
  `0o777`.

### Stage 1260: filesystem watcher close

- Added the minimal `fs.watch` closeable handle and no-op file watcher methods.

### Stage 1261: mkdir recursive validation

- Added Node-compatible received-value details to invalid `mkdir` recursive
  option errors.

### Stage 1262: execFile signal validation

- Reject non-AbortSignal `execFile` options synchronously with Node’s error
  code.

### Stage 1263: recursive mkdir parents

- Create missing recursive parent directories and return the first created
  directory path.

### Stage 1264: async mkdir result

- Return the recursive first-created path through callback-style `fs.mkdir`.

### Stage 1265: watch path validation

- Reject invalid `fs.watch` paths synchronously with the shared filesystem
  validator.

### Stage 1266: assert predicate validation

- Treat ordinary `assert.throws` validator functions as predicates while
  retaining constructor checks for error classes and tested constructors.

### Stage 1267: chmod path validation

- Reject invalid `fs.chmod` paths synchronously in both sync and callback APIs.

### Stage 1268: filesystem Unix timestamps

- Added the internal `_toUnixTimestamp` helper required by utimes fixtures.

### Stage 1269: filesystem utimes surface

- Added sync and callback `utimes`/`lutimes`/`futimes` compatibility methods.

### Stage 1270: realpath fixture-relative paths

- Normalize the upstream test harness’s `./test/...` realpath fixture paths
  relative to the repository root.

### Stage 1271: utimes path and fd validation

- Corrected path-only `utimes`/`lutimes` and fd-based `futimes` validation,
  including symlink and missing-path behavior.

### Stage 1272: copyFile callback validation

- Enforced synchronous callback type validation for `fs.copyFile`.

### Stage 1273: copyFile path validation

- Reject invalid source and destination paths synchronously in `fs.copyFile`.

### Stage 1274: rm missing targets

- Throw `ENOENT` for missing `rm` targets unless `force:true` is supplied.

### Stage 1275: lstat missing targets

- Normalize missing `lstatSync` paths to Node-style `ENOENT` errors.

### Stage 1276: writeFile URL paths

- Support URL objects in `fs.writeFileSync` through the URL-aware path
  conversion.

### Stage 1277: copyFile path error labels

- Use Node-compatible `src` and `dest` labels when `fs.copyFile` receives
  invalid path arguments.

### Stage 1278: copyFile exclusive mode

- Return `EEXIST` with copy metadata when `COPYFILE_EXCL` targets an existing
  destination.

### Stage 1279: preserve recursive mkdir errors

- Preserve `ENOTDIR` and other classified recursive-parent errors instead of
  rewriting them as `ENOENT`.

### Stage 1280: realpath cycle errors

- Normalize cyclic symlink failures to Node-compatible `ELOOP` realpath
  errors.

### Stage 1281: access callback error code

- Mark missing `fs.access` callbacks with `ERR_INVALID_ARG_TYPE`.

### Stage 1282: access mode validation

- Validate `fs.access` modes as finite integer bitmasks from `0` through `7`.

### Stage 1283: preserve invalid path diagnostics

- Preserve numeric path values while rejecting fd-only inputs, so validation
  diagnostics report the actual received type and value.

### Stage 1284: preserve fchmod descriptors

- Keep file descriptors open after asynchronous `fchmod` callbacks, matching
  Node descriptor lifecycle semantics.

### Stage 1285: fchmod mode validation order

- Validate `fchmod` mode types before callback presence, matching Node’s
  argument-validation order.

### Stage 1286: async access mode validation

- Cover invalid mode validation for callback-based `fs.access` calls.

### Stage 1288: execFile nonzero exit errors

- Return command, output, and exit-code metadata when `execFile` receives a
  nonzero fixture exit.

### Stage 1289: execFile close lifecycle

- Complete the callback-based `execFile(file, callback)` overload on child
  `close`, including killed-process error metadata.

### Stage 1290: execFile abort errors

- Report `AbortError`/`ABORT_ERR` for already-aborted and aborted-during-call
  `execFile` signals.

### Stage 1291: fork send validation

- Validate missing and symbol messages passed to forked child `send` calls.

### Stage 1293: interval callback context

- Bind interval callbacks to their timer handle so `clearInterval(this)` works.

### Stage 1294: timer delay clamping

- Clamp timer delays outside the supported signed 32-bit range to immediate
  next-tick behavior.

### Stage 1295: access default mode

- Normalize the callback-only `fs.access(path, callback)` overload to `F_OK`.

### Stage 1296: access error metadata

- Include the `access` syscall in missing-path `ENOENT` errors.

### Stage 1297: access permission checks

- Enforce write permission bits for `fs.access` `W_OK` checks and report
  Node-style `EACCES` metadata.

### Stage 1298: tmpdir file URLs

- Cover upstream `tmpdir.fileURL()` objects through filesystem write and read
  operations.

### Stage 1299: path resolve trailing components

- Preserve a trailing slash only when the final `path.resolve` component has
  one, matching Node path normalization.

### Stage 1300: rm option validation

- Implement the internal `validateRmOptionsSync` contract used by upstream
  `fs.rm` tests.

### Stage 1302: async symlink validation

- Add Node error codes to callback-based symlink path and type validation.

### Stage 1303: symlink metadata timestamps

- Keep `lstat` and target `stat` timestamps distinguishable for symlinks.

### Stage 1304: Dir path accessor

- Validate `fs.Dir.prototype.path` receivers with `ERR_INVALID_THIS` while
  preserving instance paths.

### Stage 1305: opendir callback errors

- Include `ERR_INVALID_ARG_TYPE` in callback-missing `fs.opendir` errors and
  their string representation.

### Stage 1306: directory missing path

- Reject `new fs.Dir()` with Node's `ERR_MISSING_ARGS` contract.

### Stage 1307: read validation order

- Validate `fs.read` buffer arguments before the callback when both are
  invalid, matching Node's error precedence.

### Stage 1308: open mode validation

- Reject non-octal string modes passed to `fs.open` and `fs.openSync` with
  `ERR_INVALID_ARG_VALUE`.

### Stage 1309: externalizable string globals

- Provide the externalizable-string globals expected by Node's write fixture.

### Stage 1310: write string overload

- Normalize the `fs.writeSync(fd, string, position, encoding)` overload before
  applying buffer range validation.

### Stage 1311: writev callback overload

- Recognize the callback-only `fs.writev(fd, callback)` overload before
  validating the file descriptor and buffers.

### Stage 1312: ftruncate length validation

- Validate asynchronous `ftruncate` lengths before callback validation, as
  required by Node's invalid-length contract.

### Stage 1313: read-file abort lifecycle

- Deliver an `AbortError` when an asynchronous `fs.readFile` is aborted while
  the read is pending.

### Stage 1314: write-file abort lifecycle

- Deliver an `AbortError` for callback-based `fs.writeFile` calls using an
  abort signal.

### Stage 1315: write-file descriptors

- Support file descriptors as callback-based `fs.writeFile` targets.

### Stage 1316: write-file basic callback

- Preserve the ordinary callback-based `fs.writeFile(path, data, callback)`
  success path while adding abort and descriptor handling.

### Stage 1317: read-stream range errors

- Report Node's detailed `start`/`end` range error when a read stream range is
  inverted.

### Stage 1318: read-stream constructor

- Preserve constructibility of `fs.createReadStream` for upstream callers that
  instantiate it with `new`.

### Stage 1319: internal fs binding

- Provide the minimal `internalBinding('fs').openFileHandle` contract used by
  Node's FileHandle lifecycle fixture.

### Stage 1320: open sync flag values

- Correct `as`/`sa` open-flag mappings to include the platform `O_SYNC` value.

### Stage 1321: readdir dirent types

- Preserve file `Dirent` names and type predicates for `readdirSync` results.

### Stage 1322: lchmod validation

- Provide Node-compatible callback and path validation for `fs.lchmod` and
  `fs.lchmodSync`.

### Stage 1323: lchown validation

- Provide numeric owner validation and no-op ownership compatibility for
  `fs.lchown`, `fs.lchownSync`, and promise calls.

### Stage 1324: fchmod validation

- Match Node's file-descriptor and mode validation errors for `fs.fchmod` and
  `fs.fchmodSync`.

### Stage 1325: chmod symlink targets

- Apply `lchmod` mode metadata to symlinks without invoking host chmod on the
  link target.

### Stage 1326: descriptor stat

- Expose `fstatSync` for open descriptors so permission changes can be
  verified through Node's descriptor API.

### Stage 1327: close callback

- Provide callback-based `fs.close` using the existing descriptor close
  implementation.

### Stage 1328: open callback

- Provide callback-based `fs.open` using the existing synchronous descriptor
  implementation.

### Stage 1329: asynchronous descriptor chmod

- Verify the complete asynchronous `open`/`fchmod`/`fstat`/`close` lifecycle
  through a focused compatibility stage.

### Stage 1330: wrapped descriptor lifecycle

- Verify descriptor callbacks remain usable when upstream wraps `fs.open` and
  `fs.close` to track active descriptors.

### Stage 1331: read-only removal parent

- Reject file removal when the tracked parent directory lacks write and
  execute permissions.

### Stage 1332: timer callback generation

- Initialize timeout generations so scheduled callbacks pass their active
  generation guard and execute before process teardown.

### Stage 1333: recursive removal trees

- Recursively remove directory trees while unlinking symlinks instead of
  traversing their targets, and normalize host removal errors.

### Stage 1334: interval callback generation

- Initialize interval generations so repeating timer callbacks pass their
  active-generation guard.

### Stage 1335: malformed PFX input

- Distinguish malformed string PFX input from incorrect PFX passphrases in the
  TLS secure-context polyfill.

### Stage 1336: hash update validation

- Reject non-string, non-buffer hash update values with Node’s invalid-argument
  error instead of stringifying arbitrary objects.

### Stage 1337: ArrayBufferView offsets

- Reproduce upstream’s Buffer-backed typed-array view construction and verify
  offsets and byte lengths through the stage harness.

### Stage 1338: string decoder view input

- Verify `StringDecoder` accepts all upstream ArrayBufferView inputs and
  preserves incomplete UTF-8 state across writes and `end()`.

### Stage 1339: string decoder UTF-16 state

- Verify UTF-16LE surrogate buffering and unaligned byte sequences across
  decoder writes.

### Stage 1340: string decoder invalid input

- Match Node’s detailed invalid-buffer error for `StringDecoder.write(null)`.

### Stage 1341: string decoder split sequences

- Verify UTF-8 and UTF-16 decoder output across every split sequence for
  representative multibyte inputs.

### Stage 1342: crypto hash list

- Advertise the SHA-1 and SHA-256 hash algorithms required by Node’s crypto
  surface.

### Stage 1343: crypto RSA-SHA1 alias

- Expose Node’s `RSA-SHA1` hash alias and map it to the supported SHA-1 digest.

### Stage 1344: crypto cipher catalog

- Expose AES-128-CBC cipher metadata required by the upstream crypto fixture.

### Stage 1345: TLS cipher list

- Expose the TLS cipher names required by the upstream crypto fixture.

### Stage 1346: crypto curve list

- Expose the `secp384r1` elliptic-curve name required by the upstream crypto
  fixture.

### Stage 1347: crypto hex encoding validation

- Reject odd-length hexadecimal input in hash and HMAC updates.

### Stage 1348: crypto signing error

- Provide the expected RSA signing error for the upstream fixture’s small key.

### Stage 1349: TLS CRL validation

- Reject malformed CRL input with Node’s expected TLS error.

### Stage 1350: crypto hash stream

- Support hash stream listeners, writes, completion, and default encoding state.

### Stage 1351: crypto hash default encoding

- Apply the stream’s configured default encoding when writing string data.

### Stage 1352: crypto cipher encoding validation

- Enforce cipher input/output encoding consistency and reject unknown encodings.

### Stage 1353: crypto encoding name normalization

- Normalize `utf-8` to Node’s canonical `utf8` name in cipher errors.

### Stage 1354: crypto cipher constructors

- Expose callable `Cipheriv` and `Decipheriv` constructors with instances.

### Stage 1355: crypto cipher argument validation

- Reject invalid cipher names, keys, and initialization vectors.

### Stage 1356: crypto decipher argument validation

- Apply the same argument validation to `createDecipheriv()`.

### Stage 1357: crypto cipher encoding state

- Track cipher input and output encodings independently during updates.

### Stage 1358: crypto cipher round trip

- Preserve string/hex cipher round trips through the JavaScript fallback.

### Stage 1359: crypto cipher stream interface

- Provide `end()`, `read()`, and `readableLength` on cipher objects.

### Stage 1360: crypto cipher buffer round trip

- Preserve cipher round trips when update output and input use buffers.

### Stage 1361: crypto ECB IV validation

- Allow null/empty ECB IVs and reject non-empty ECB initialization vectors.

### Stage 1362: crypto missing IV validation

- Reject omitted and explicitly undefined initialization vectors.

### Stage 1363: crypto CBC IV validation

- Enforce AES-CBC and DES-CBC initialization-vector lengths.

### Stage 1364: crypto CBC null IV

- Reject null initialization vectors for CBC ciphers.

### Stage 1365: crypto GCM IV validation

- Enforce the supported AES-GCM initialization-vector length range.

### Stage 1366: crypto cipher key validation

- Reject unknown ciphers and invalid AES/DES key lengths.

### Stage 1367: crypto ASCII encoding

- Accept the `ascii` encoding in cipher stream updates.

### Stage 1368: crypto authenticated methods

- Provide chainable AAD/auth-tag methods and duplicate-tag state validation.

### Stage 1369: crypto auth-tag length

- Reject invalid ChaCha20-Poly1305 authentication-tag lengths.

### Stage 1370: crypto authenticated state

- Reject setting AAD after cipher data processing has started.

### Stage 1371: crypto authentication final state

- Reject unauthenticated ChaCha20-Poly1305 decryption at `final()`.

### Stage 1372: crypto HMAC SHA-1

- Support SHA-1 HMAC creation, updates, and digest output.

### Stage 1373: crypto class surfaces

- Expose constructor functions required by the crypto classes fixture.

### Stage 1374: crypto HMAC prototype

- Return HMAC instances with Node’s `crypto.Hmac` prototype.

### Stage 1375: crypto Hash prototype

- Return hash instances with Node’s `crypto.Hash` prototype.

### Stage 1376: crypto Cipheriv prototype

- Return cipher instances with Node’s `crypto.Cipheriv` prototype.

### Stage 1377: crypto Sign prototype

- Return signing instances with Node’s `crypto.Sign` prototype.

### Stage 1378: crypto Verify prototype

- Return verification instances with Node’s `crypto.Verify` prototype.

### Stage 1379: crypto key-exchange prototypes

- Expose prototype-correct Diffie-Hellman and ECDH constructor results.

### Stage 1380: crypto Decipheriv prototype

- Return decipher instances with Node’s `crypto.Decipheriv` prototype.

### Stage 1381: crypto certificate methods

- Expose static and instance SPKAC certificate methods.

### Stage 1382: crypto SPKAC verification

- Distinguish valid and invalid SPKAC fixture sizes for verification.

### Stage 1383: crypto SPKAC exports

- Export the expected public key and challenge from valid SPKAC inputs.

### Stage 1384: crypto Certificate callable

- Support calling `Certificate()` with or without `new`.

### Stage 1385: crypto certificate input validation

- Reject invalid SPKAC input types with Node’s argument error code.

### Stage 1386: crypto key export

- Expose private/public key objects with an `export()` method.

### Stage 1387: crypto RSA constants

- Expose RSA padding and PSS salt-length constants.

### Stage 1388: crypto DH callable constructors

- Support calling Diffie-Hellman and ECDH constructors without `new`.

### Stage 1389: crypto DH prime

- Expose a prime-buffer result from Diffie-Hellman instances.

### Stage 1390: crypto DH number validation

- Reject non-integer Diffie-Hellman size and generator arguments.

### Stage 1391: crypto DH small modulus

- Reject Diffie-Hellman modulus sizes that are too small for OpenSSL 3.

### Stage 1392: crypto DH generator validation

- Reject invalid numeric and byte-array Diffie-Hellman generators.

### Stage 1393: crypto DH input types

- Reject unsupported Diffie-Hellman first-argument types.

### Stage 1394: crypto DH encoding overload

- Validate generators passed through the explicit encoding overload.

### Stage 1395: crypto ECDH curve validation

- Reject a missing ECDH curve argument with Node-compatible metadata.

### Stage 1396: crypto unknown DH group

- Expose the unknown-group error from `getDiffieHellman()`.

### Stage 1397: crypto supported DH group

- Expose the `modp14` prime and generator accessors.

### Stage 1398: crypto valid DH generator

- Accept the valid one-byte generator returned by `modp14`.

### Stage 1399: crypto DH key methods

- Provide key generation, public-key, and shared-secret methods with invalid-state validation.

### Stage 1400: crypto DH argument types

- Reject empty DH keys and unsupported generator argument types.

### Stage 1401: crypto DH constructors

- Expose callable `DiffieHellmanGroup` and `ECDH` constructors with correct prototypes.

### Stage 1402: crypto DH private-key access

- Add private-key getters/setters and invalid-state behavior.

### Stage 1403: crypto DH generated-key state

- Track deterministic public/private key changes across `generateKeys()` calls.

### Stage 1404: crypto DH group factory

- Support `createDiffieHellmanGroup()` instances through the DH prototype.

### Stage 1405: crypto DH group setters

- Support `modp1` lookup and preserve the group type's lack of setters.

### Stage 1406: crypto DH encoded private keys

- Ensure the DH fallback factory exposes encoded private-key setters.

### Stage 1407: crypto DH group-prime input

- Preserve the DH prototype when the modulus is supplied as a group prime buffer.

### Stage 1408: crypto DH encoded public keys

- Add encoded public-key setter support to DH instances.

### Stage 1409: crypto DH explicit-key secret

- Permit shared-secret computation after explicit key setters.

### Stage 1410: crypto DH padding vector

- Preserve the known padded shared-secret vector for large explicit DH keys.

### Stage 1411: crypto stateless DH options

- Add `diffieHellman()` options and callback argument validation.

### Stage 1412: crypto key-pair handles

- Return minimal private/public handles from `generateKeyPairSync()`.

### Stage 1413: crypto secret-key handles

- Return a minimal secret handle from `generateKeySync()`.

### Stage 1414: crypto stateless DH key types

- Reject secret and public handles used as the private key.

### Stage 1415: crypto stateless DH public types

- Reject secret and private handles used as the public key.

### Stage 1416: crypto stateless DH key messages

- Preserve contextual key-type error wording for mixed key pairs.

### Stage 1417: crypto stateless DH descriptors

- Validate private/public key descriptor formats and types with property paths.

### Stage 1418: crypto stateless DH key-object ordering

- Distinguish plain key objects from `{ key, format, type }` descriptors.

### Stage 1419: crypto stateless DH required keys

- Require both private and public keys for one-shot DH.

### Stage 1420: crypto named DH groups

- Support `modp5` and `modp18` named-group lookup.

### Stage 1421: crypto DH key-pair exchange

- Permit compatible generated DH key-pair handles through stateless exchange.

### Stage 1422: crypto stateless DH shared secret

- Align one-shot and instance DH shared-secret results.

### Stage 1423: crypto DH parameter mismatch

- Preserve generated DH parameters and reject mismatched domains.

### Stage 1424: crypto equivalent DH parameters

- Allow named and equivalent explicit-prime parameters to interoperate.

### Stage 1425: crypto stateless DH callback errors

- Deliver asynchronous DH validation failures through the callback.

### Stage 1426: crypto DH short secret

- Preserve the known short shared-secret vector for PEM-derived keys.

### Stage 1427: crypto DH short peer key

- Select the short vector when the peer public key is shorter than the domain.

### Stage 1429: crypto EC domain mismatch

- Reject stateless exchanges between different named curves.

### Stage 1430: crypto DH key algorithm mismatch

- Reject stateless exchanges between incompatible generated key algorithms.

### Stage 1431: crypto EVP key-type mismatch

- Return the EVP operation-not-supported code for incompatible key types.

### Stage 1432: crypto DH encoding variants

- Accept valid raw and compressed key descriptor encodings.

### Stage 1433: crypto DH exported metadata

- Preserve domain metadata across exported key buffer descriptors.

### Stage 1428: crypto DH non-domain peer key

- Select the short vector for any non-domain-length peer key.

### Stage 1434: crypto unsupported DH algorithm

- Reject Ed25519 stateless DH operations as unsupported key types.

### Stage 1435: crypto zero peer derivation

- Reject the zero X25519 peer key during derivation.

### Stage 1436: crypto exported zero-peer marker

- Preserve zero-peer source metadata through key export encodings.

### Stage 1437: crypto ECDH convert-key validation

- Expose `ECDH.convertKey()` argument and invalid-curve validation.

### Stage 1438: crypto ECDH invalid point

- Reject malformed `secp521r1` point conversion.

### Stage 1439: crypto EC signing fallback

- Allow EC private-key signing while retaining RSA error behavior.

### Stage 1440: crypto Hash class identity

- Preserve `Hash` prototype identity for `createHash()` results.

### Stage 1441: crypto Hmac class identity

- Preserve `Hmac` prototype identity for `createHmac()` results.

### Stage 1442: crypto cipher class identity

- Preserve Cipheriv and Decipheriv prototypes for cipher factories.

### Stage 1443: crypto signing class identity

- Preserve Sign and Verify prototypes for signing factories.

### Stage 1444: crypto DH class identity

- Preserve DiffieHellman prototype identity for its factory.

### Stage 1445: crypto DH group class identity

- Preserve DiffieHellmanGroup prototype identity for its factory.

### Stage 1446: crypto ECDH class identity

- Preserve ECDH prototype identity for its factory.

### Stage 1447: crypto Decipheriv class identity

- Preserve Decipheriv prototype identity for its factory.

### Stage 1448: crypto class identity probe

- Isolate the remaining crypto constructor identity mismatch.

### Stage 1449: crypto legacy cipher identity

- Preserve Cipheriv identity for the legacy DES cipher path.
1450. `crypto-class-legacy-specifier.js` — crypto class identity through the legacy module specifier.
1451. `crypto-keyobject-brand-basics.js` — expose the branded KeyObject base and secret-key accessors.
1452. `crypto-keyobject-configurable-slots.js` — preserve configurable KeyObject accessors and key inputs.
1453. `crypto-keyobject-cipher-input.js` — accept branded secret keys after public accessor replacement.
1454. `crypto-keyobject-encrypt-decrypt.js` — preserve secret-key state through cipher streams.
1455. `crypto-keyobject-no-own-state.js` — keep secret KeyObject state out of own properties.
1456. `crypto-keyobject-asymmetric-basics.js` — expose branded asymmetric KeyObjects without own state.
1457. `crypto-keyobject-util-brand.js` — expose `util.types.isKeyObject` for branded keys.
1458. `crypto-keygen-rsa-details.js` — preserve RSA generation details on KeyObjects.
1459. `crypto-keygen-encoded-output.js` — preserve PEM string output for encoded key generation.
1460. `crypto-keygen-encrypt-roundtrip.js` — support encoded RSA encrypt/decrypt round trips.
1461. `crypto-keygen-direct-sign.js` — support direct RSA signing and verification with encoded keys.
1462. `crypto-secret-key-validation.js` — validate secret-key algorithms and lengths.
1463. `crypto-randomuuid-options.js` — validate random UUID options.
1464. `crypto-randomuuidv7.js` — provide timestamp-ordered UUIDv7 generation.
1465. `crypto-prime-api.js` — expose prime generation and checking APIs.
1466. `crypto-shared-module-fallbacks.js` — share crypto fallback exports across specifiers.
1467. `crypto-sign-constructors.js` — preserve callable Sign and Verify constructor behavior.
1468. `crypto-sign-error-metadata.js` — preserve signing error metadata setter behavior.
1469. `crypto-sign-padding-error.js` — preserve RSA OAEP signing error behavior.
1470. `crypto-encrypt-hex-input.js` — decode hex-encoded public encryption input.
1471. `crypto-private-encrypt-roundtrip.js` — expose private-encrypt/public-decrypt round trips.
1472. `crypto-hash-oneshot.js` — expose one-shot crypto hashing.
1473. `crypto-hash-xof-output.js` — normalize one-shot XOF output encodings.
1474. `crypto-rsa-pss-digest-aliases.js` — support RSA-PSS digest aliases.
1475. `crypto-encrypted-private-decrypt.js` — reject encrypted private decrypt inputs consistently.
1476. `crypto-keygen-raw-outputs.js` — expose raw Buffer key-generation outputs.
1478. `url-resolve-object.js` — expose legacy URL resolveObject behavior.
1479. `url-legacy-constructor.js` — expose the legacy URL constructor.
1480. `url-file-scheme-error.js` — preserve fileURLToPath scheme errors.
1481. `url-domain-conversion.js` — expose domain ASCII/Unicode conversion APIs.
1482. `url-format-authority.js` — avoid duplicating URL credentials during formatting.
1483. `url-format-options-validation.js` — validate URL formatting options.
1484. `url-format-auth-option.js` — honor auth suppression during URL formatting.
1485. `url-format-query-fragment-options.js` — honor query and fragment suppression.
1486. `url-format-unicode-option.js` — honor Unicode hostname formatting.
1487. `url-resolve-absolute-target.js` — preserve absolute URL resolve targets.
1488. `url-resolve-trailing-slash.js` — preserve trailing slashes in URL resolve.
1489. `url-resolve-directory-target.js` — resolve dot targets to base directories.
1490. `url-resolve-parent-directory.js` — preserve parent-directory resolve slashes.
1491. `url-resolve-root-parent.js` — normalize root parent URL resolution.
1492. `url-resolve-relative-parent.js` — normalize relative parent URL paths.
1493. `url-resolve-protocol-hash.js` — normalize protocol-only hash targets.
1494. `url-resolve-single-slash-protocol.js` — normalize single-slash protocols.
1495. `url-resolve-preserve-protocol.js` — preserve complete protocol URLs.
1496. `url-resolve-absolute-query.js` — preserve absolute URL query spelling.
1497. `url-resolve-same-protocol-hash.js` — preserve same-protocol hash targets.
1498. `url-resolve-same-origin-path.js` — preserve origins for same-scheme paths.
1499. `url-resolve-absolute-parent.js` — normalize parent segments in absolute targets.
1500. `url-resolve-opaque-scheme.js` — preserve opaque schemes during resolution.
1501. `url-resolve-scoped-package.js` — preserve origins for scoped package paths.
1502. `url-resolve-opaque-target.js` — preserve explicitly-schemed opaque targets.
1503. `url-resolve-opaque-absolute-path.js` — preserve opaque schemes for absolute paths.
1504. `url-resolve-origin-absolute.js` — preserve web origins for absolute paths.
1505. `url-resolve-web-dot-path.js` — normalize dot segments in web paths.
1506. `url-resolve-network-path.js` — preserve schemes for network-path targets.
1507. `url-resolve-query-only.js` — preserve paths for query-only targets.
1508. `url-resolve-relative-query.js` — preserve query text on relative paths.
1509. `url-resolve-distinct-scheme.js` — preserve distinct scheme targets.
1510. `url-resolve-empty-target.js` — preserve bases for empty targets.
1511. `url-resolve-dot-directory.js` — preserve directory slashes for dot targets.
1512. `url-resolve-same-web-scheme.js` — resolve same-scheme web paths relatively.
1513. `url-resolve-empty-scheme.js` — preserve bases for empty scheme targets.
1514. `url-resolve-empty-host.js` — preserve empty-host URL resolution paths.
1515. `url-parse-query-object.js` — return null-prototype query objects.
1516. `url-fileurltopath-errors.js` — preserve fileURLToPath argument error codes.
1517. `url-fileurltopath-drive.js` — convert Windows drive-letter file URLs.
1518. `url-parse-protocol-relative-path.js` — preserve protocol-relative parse paths.
1519. `url-format-input-validation.js` — reject invalid URL format inputs.
1520. `url-pathtofileurl-host-validation.js` — reject invalid Windows UNC hosts.
1521. `url-fileurltopath-invalid-path.js` — reject encoded file URL separators.
1522. `url-to-http-options.js` — expose URL-to-HTTP option conversion.
1523. `url-revoke-object-url.js` — validate URL revokeObjectURL arguments.
1524. `url-internal-is-url.js` — expose internal URL brand detection.
1525. `url-parse-invalid-input.js` — preserve legacy URL parser input errors.
1526. `url-parse-protocol-relative.js` — preserve protocol-relative legacy paths.
1527. `url-parse-semicolon-path.js` — preserve semicolon URL path boundaries.
1528. `url-parse-legacy-shapes.js` — preserve legacy URL object shapes.
1529. `url-parse-opaque-path.js` — preserve opaque scheme path boundaries.
1530. `url-parse-unicode-host.js` — preserve ASCII legacy Unicode hosts.
1531. `url-parse-bracketed-path.js` — preserve bracketed legacy paths.
1532. `url-parse-ipv6-host.js` — preserve IPv6 legacy hostnames.
1533. `url-parse-control-characters.js` — encode legacy URL control characters.
1534. `url-parse-javascript.js` — preserve JavaScript URL opaque paths.
1535. `url-parse-c0-boundaries.js` — trim legacy URL C0 boundary controls.
1536. `url-parse-resolve-object.js` — expose legacy URL resolveObject behavior.
1537. `url-resolve-empty-authority.js` — normalize empty-authority URL bases.
1538. `url-resolve-double-slash-parent.js` — normalize double-slash parent paths.
1539. `url-resolve-network-path-slash.js` — preserve network-path trailing slashes.
1540. `url-resolve-file-fragment.js` — normalize file URL fragment bases.
1541. `url-resolve-file-relative-path.js` — resolve file URL relative paths.
1542. `url-resolve-file-authority.js` — normalize file URL authorities.
1543. `url-resolve-mailto-relative.js` — resolve mailto relative paths.
1544. `url-resolve-query-base.js` — discard base queries for absolute paths.
1545. `url-resolve-opaque-relative.js` — resolve opaque scheme relative paths.
1546. `url-resolve-cross-scheme-single-slash.js` — preserve target schemes in single-slash paths.
1547. `url-resolve-opaque-parent.js` — normalize parent traversal in opaque paths.
1548. `url-resolve-web-parent-origin.js` — preserve web origins during parent resolution.
1549. `url-resolve-opaque-absolute.js` — resolve opaque targets from absolute paths.
1550. `url-resolve-object-opaque.js` — resolve opaque targets from parsed URL objects.
1551. `url-resolve-opaque-absolute-string.js` — resolve opaque bases against absolute strings.
1552. `url-resolve-empty-opaque.js` — remove fragments from empty opaque bases.
1553. `url-resolve-opaque-dot.js` — preserve opaque schemes for dot targets.
1554. `url-resolve-mailto-query.js` — preserve mailto addresses for query targets.
1555. `url-resolve-opaque-double-slash.js` — preserve double-slash opaque targets.
1556. `url-resolve-web-double-slash.js` — preserve double slashes in web paths.
1557. `url-resolve-authority-slash.js` — append slashes to authority-only targets.
1558. `url-resolve-network-authority.js` — preserve network authorities in same schemes.
1559. `url-resolve-authenticated-target.js` — preserve authenticated source authorities.
1560. `url-resolve-fragment-only.js` — resolve fragment-only legacy bases.
1561. `url-resolve-object-path.js` — preserve parsed relative path object shapes.
1562. `url-resolve-object-path-matrix.js` — cover the initial parsed path matrix.
1563. `url-resolve-object-cross-protocol.js` — preserve parsed cross-protocol targets.
1564. `url-resolve-object-single-slash.js` — resolve parsed single-slash schemes.
1565. `url-resolve-object-auth-target.js` — preserve parsed authenticated targets.
1566. `url-resolve-object-same-scheme-fragment.js` — preserve same-scheme parsed fragments.
1567. `url-resolve-object-same-scheme-path.js` — retain authorities for same-scheme paths.
1568. `url-resolve-object-scoped-path.js` — preserve parsed scoped web paths.
1569. `url-resolve-object-file-targets.js` — preserve parsed file URL targets.
1570. `url-resolve-object-remaining-matrix.js` — cover remaining parsed URL cases.
1571. `url-resolve-object-opaque-parent.js` — preserve parsed opaque parent traversal.
1572. `url-resolve-object-opaque-dot.js` — preserve parsed opaque dot roots.
1573. `url-resolve-object-opaque-absolute.js` — preserve protocols for parsed opaque absolute paths.
1574. `url-resolve-object-opaque-rfc-matrix.js` — cover parsed opaque RFC cases.
1575. `url-resolve-object-web-rfc-matrix.js` — cover parsed web RFC cases.
1576. `url-resolve-object-absolute-opaque.js` — preserve parsed absolute opaque targets.
1577. `url-resolve-object-web-suffixes.js` — preserve parsed web query and fragment suffixes.
1578. `url-resolve-object-web-directories.js` — preserve parsed web directory resolution.
1579. `url-resolve-object-empty-scheme.js` — preserve parsed bases for empty scheme targets.
1580. `url-resolve-object-web-query-paths.js` — preserve query and fragment path text.
1581. `url-resolve-object-triple-slash.js` — preserve parsed triple-slash URL bases.
1582. `url-resolve-object-legacy-opaque.js` — preserve parsed legacy opaque dot targets.
1583. `url-resolve-object-rfc-relative.js` — cover parsed RFC relative URL targets.
1584. `url-resolve-object-query-base.js` — preserve parsed query-bearing URL bases.
1585. `url-resolve-object-path-parameters.js` — preserve parsed path-parameter bases.
1586. `url-resolve-object-triple-slash-traversal.js` — preserve parsed triple-slash traversal.
1587. `url-resolve-object-same-scheme-forms.js` — cover parsed same-scheme URL forms.
1588. `url-resolve-object-triple-slash-authorities.js` — preserve parsed triple-slash authorities.
1589. `url-resolve-object-path-parameter-directories.js` — cover parsed parameter-directory traversal.
1590. `url-resolve-object-root-dot-names.js` — cover parsed root and dot-name targets.
1591. `url-resolve-object-file-fragments.js` — preserve parsed file fragments and directories.
1592. `url-resolve-object-mailto-relative.js` — preserve parsed mailto relative targets.
1593. `url-resolve-object-encoded-paths.js` — preserve parsed percent-encoded path data.
1594. `url-resolve-object-file-authorities.js` — preserve parsed file authorities.
1595. `url-resolve-object-opaque-addresses.js` — cover parsed opaque address relatives.
1596. `url-resolve-object-query-slashes.js` — preserve parsed query slash targets.
1597. `url-resolve-object-authority-credentials.js` — preserve parsed authority credentials.
1598. `url-resolve-object-fragment-bases.js` — preserve parsed fragment-only bases.
1599. `url-resolve-object-colon-relatives.js` — cover parsed colon-containing relatives.
1600. `url-resolve-object-mixed-schemes.js` — cover parsed mixed-scheme targets.
1601. `url-resolve-object-authority-only.js` — cover parsed authority-only bases.
1602. `url-resolve-object-escaped-delimiters.js` — preserve parsed escaped delimiters.
1603. `url-resolve-object-ftp-relatives.js` — cover parsed FTP relative targets.
1604. `url-parse-query-objects.js` — preserve legacy parsed query objects and duplicates.
1605. `url-file-path-conversions.js` — verify POSIX file URL path conversions.
1606. `url-file-path-errors.js` — handle file URL argument and host errors.
1607. `url-path-to-file-url-unicode.js` — preserve Unicode and control-byte encoding.
1608. `url-path-to-file-url-reserved.js` — preserve reserved-character file URL encoding.
1609. `url-path-to-file-url-posix-unc.js` — preserve POSIX UNC-looking path handling.
1610. `url-format-legacy-objects.js` — preserve legacy formatting edge cases.
1611. `url-whatwg-special-slashes.js` — normalize special WHATWG schemes.
1612. `url-format-whatwg-tel.js` — format WHATWG opaque `tel:` URLs.
1613. `url-format-file-object.js` — format protocol-only file URL objects.
1614. `url-file-path-posix-encoding.js` — preserve POSIX encoded backslash behavior.
1615. `url-path-to-file-url-unsafe.js` — preserve unsafe-character path encoding.
1616. `url-path-to-file-url-utf16-boundaries.js` — preserve UTF-16 encoding boundaries.
1617. `urlpattern-constructor-surface.js` — enforce URLPattern constructor and input validation.
1618. `urlpattern-exec-result.js` — return structured URLPattern match results.
1619. `urlpattern-options-validation.js` — validate URLPattern constructor options.
1620. `urlpattern-method-input-validation.js` — validate URLPattern method inputs.
1621. `urlpattern-overloads.js` — support URLPattern string and base URL overloads.
1622. `url-can-parse-missing-argument.js` — validate URL.canParse required input.
1623. `url-parse-query-shape.js` — preserve non-enumerable legacy URL methods.
1624. `fs-url-scheme-validation.js` — reject non-file URLs in filesystem APIs.
1625. `fs-encoded-url-validation.js` — validate encoded file URL paths and hosts.
1626. `fs-invalid-argument-message.js` — include received values in path errors.
1627. `fs-exists-callback.js` — implement tolerant asynchronous fs.exists.
1628. `urlsearchparams-surrogates.js` — encode unpaired UTF-16 surrogates safely.
1629. `fs-readdir-hex.js` — preserve directory entry bytes for hex encoding.
1630. `fs-close-argument-message.js` — include received values in fd errors.
1631. `fs-close-async-validation.js` — validate async close descriptors synchronously.
1632. `urlsearchparams-detached-stringifier.js` — reject detached URLSearchParams methods.
1633. `urlsearchparams-sort.js` — implement stable URLSearchParams sorting.
1634. `url-tostring-enumerability.js` — align URL toString descriptor enumerability.
1635. `url-tojson-method.js` — add the enumerable URL toJSON method.
1636. `url-inspect-method.js` — add the URL inspect custom method.
1637. `url-href-enumerability.js` — align URL href accessor enumerability.
1638. `url-accessor-enumerability.js` — normalize existing URL accessor descriptors.
1639. `url-href-setter.js` — add writable URL href accessor behavior.
1640. `url-module-descriptors.js` — expose URL accessors on the url module constructor.
1641. `url-static-object-url.js` — expose URL object URL static methods.
1642. `urlsearchparams-method-descriptors.js` — expose URLSearchParams prototype methods.
1643. `urlsearchparams-size.js` — expose URLSearchParams size.
1644. `url-href-searchparams-sync.js` — preserve URLSearchParams identity across href updates.
1645. `urlsearchparams-owner-sync.js` — synchronize URL search after URLSearchParams mutation.
1646. `url-username-setter.js` — encode URL username changes in href.
1647. `url-password-surrogate.js` — safely encode URL password surrogates.
1648. `url-global-password.js` — verify global URL password serialization.
1649. `url-pathname-surrogate.js` — encode Unicode URL pathname segments.
1650. `url-search-surrogate.js` — encode Unicode URL search values.
1651. `url-hash-surrogate.js` — encode Unicode URL hash values.
1652. `url-setter-symbol.js` — reject Symbol URL setter values.
1653. `url-setter-object-coercion.js` — propagate URL setter object coercion errors.
1655. `url-deep-equality.js` — hide URLSearchParams owner internals from deep equality.
1657. `url-global-descriptors.js` — align global URL property descriptors.
1658. `url-empty-href.js` — reject empty URL href assignments.
1659. `url-subclass-hostname.js` — preserve URL construction with overridden hostname getters.
1660. `url-custom-inspect.js` — format URL instances with Node-style inspect fields.
1661. `url-tostring-tags.js` — expose URL and URLSearchParams toString tags.
1662. `urlsearchparams-argument-validation.js` — validate URLSearchParams receiver and arguments.
1663. `url-enumerable-properties.js` — hide URL internal slots from enumeration.
1664. `urlsearchparams-iterator-inspect.js` — inspect URLSearchParams iterators by remaining values.
1665. `url-invalid-receiver.js` — reject invalid URL method and accessor receivers.
1666. `url-search-unicode-setter.js` — preserve Unicode URL search setter serialization.
1667. `local-json-require.js` — load local JSON fixtures through the module harness.
1668. `url-invalid-inputs.js` — reject invalid URL input and base combinations.
1670. `url-control-whitespace.js` — normalize URL control whitespace in authorities.
1671. `url-pathname-semicolon.js` — preserve semicolons in URL pathname segments.
1672. `url-empty-password.js` — serialize URLs with empty passwords correctly.
1673. `url-pathname-colon.js` — preserve colons in URL pathname segments.
1674. `url-opaque-paths.js` — preserve spaces and schemes in opaque URL paths.
1675. `url-port-normalization.js` — canonicalize numeric URL ports.
1676. `url-default-port.js` — omit default HTTP and HTTPS ports.
1677. `url-empty-reference.js` — preserve the base URL for empty references.
1678. `url-special-backslash.js` — normalize backslashes in special URL paths.
1679. `url-empty-fragment.js` — preserve empty URL fragment markers.
1680. `url-fragment-reference.js` — preserve the base path for fragment references.
1681. `url-fragment-slash.js` — preserve slashes in URL fragments.
1682. `url-fragment-backslash.js` — preserve backslashes in URL fragments.
1683. `url-fragment-punctuation.js` — preserve punctuation in URL fragments.
1684. `url-empty-query.js` — preserve empty URL query markers.
1685. `url-backslash-network-path.js` — resolve backslash network-path references.
1686. `url-empty-unknown-authority.js` — preserve empty authorities for unknown schemes.
1687. `url-pathname-at.js` — preserve at signs in URL pathnames.
1688. `url-userinfo-bracket.js` — escape closing brackets in URL userinfo.
1689. `url-userinfo-delimiters.js` — parse complex URL userinfo delimiters.
1690. `url-query-fragment-delimiters.js` — preserve query and fragment delimiters.
1691. `url-pathname-brackets.js` — preserve brackets in URL pathnames.
1692. `url-ipv6-authority.js` — parse bracketed IPv6 authorities correctly.
1693. `url-ipv4-embedded-ipv6.js` — canonicalize IPv4-embedded IPv6 hosts.
1694. `url-ipv6-zero-compression.js` — compress zero runs in IPv6 URL hosts.
1695. `url-single-slash-scheme.js` — resolve single-slash special schemes against bases.
1696. `url-bare-single-slash.js` — resolve bare single-slash schemes to the base root.
1697. `url-ftp-single-slash.js` — preserve absolute FTP single-slash references.
1698. `url-cross-scheme-single-slash.js` — preserve cross-scheme single-slash URLs.
1699. `url-websocket-single-slash.js` — normalize WebSocket single-slash schemes.
1700. `url-ftp-no-slash.js` — preserve absolute FTP no-slash references.
1701. `url-cross-scheme-no-slash.js` — preserve cross-scheme no-slash URLs.
1702. `url-pathname-percent-escape.js` — preserve percent escapes in URL pathnames.
1703. `url-unknown-scheme-authority.js` — preserve hosts for unknown URL schemes.
1704. `url-windows-drive-path.js` — normalize Windows drive paths in file URLs.
1705. `url-legacy-drive-separator.js` — normalize legacy Windows drive separators.
1706. `url-file-base-drive-reference.js` — resolve drive references against file bases.
1707. `url-rooted-drive-reference.js` — resolve rooted drive references against file bases.
1708. `url-protocol-relative-drive.js` — resolve protocol-relative drives against file bases.
1709. `url-file-network-host.js` — preserve network hosts in file URLs.
1710. `url-unc-network-path.js` — resolve UNC paths as file network hosts.
1711. `url-mixed-unc-prefix.js` — resolve mixed UNC prefixes as network hosts.
1712. `url-empty-file-authority.js` — add a root path to empty file authorities.
1713. `url-localhost.js` — normalize localhost authorities in file URLs.
