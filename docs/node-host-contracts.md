# Native Node host contracts

Node API policy belongs to `quench-node`; JavaScript coercions, property access,
exceptions, roots and jobs use the shared VM. Existing class/function bridges
supply observable syntax and delegate policy to native operations.

## Event listeners

EventEmitter dispatch and listener inspection use the same `_events` records.
An emission retains the selected event store across error-monitor callbacks.
Monitors dispatch through the public `emit` hook. Borrowed `emit` calls do not
require a `rawListeners` method or initialize an absent event store. Domain
forwarding preserves truthy primitive errors and decorates object errors.
The existing stream-owned unhandled-error bridge remains in use.

`getEventListeners` delegates to a callable public `listeners` hook, including
its repeated getter access. EventTarget inspection returns a fresh projection
of its records. Abort listeners use their private signal state; `onabort`
occupies an ordered wrapper whose handler can change without replacing the
wrapper. Node's observable listener count changes through registration,
removal and handler activation transitions independently of wrapper membership.

`addAbortListener` accepts Node's structural AbortSignal validation and returns
a null-prototype disposable. An already-aborted signal queues a job with no
arguments; disposal does not cancel that job. Live subscriptions use immutable
options with `once` and propagation resistance. Disposal looks up the public
removal hook each time. Dispatch snapshots registrations, skips removals made
during the dispatch, and permits propagation-resistant listeners after an
ordinary listener stops immediate propagation.

These contracts cover native abort subscriptions. General EventTarget
registration, weak-listener ownership and non-abort signal events still use
existing compatibility paths and require their own oracle evidence.

Native argument-validation helpers share an `ERR_INVALID_ARG_TYPE` prototype.
Its constructor accessor preserves the captured TypeError identity; code and
message retain Node's property order and attributes. Its `toString` captures
the error code while reading/coercing the receiver's name and message. The
private prototype brand is owned by these native errors; identity with every
constructor/brand exported by upstream `internal/errors` is not established.

## Async resources and hooks

Async execution identity uses the shared native identity owner. Creating a
resource emits init with its captured trigger identity while preserving the
caller's execution context. Manual `runInAsyncScope` exchanges execution and
trigger identities, exposes the resource to `executionAsyncResource`, emits
before/after, and restores the caller even when the callback throws.

Hook constructors copy callback slots after JavaScript parameter destructuring.
Subscriptions use a private captured Set and Uint32 callback counts. An outer
hook emission retains an array snapshot and its counts; nested emissions use
that snapshot while enable/disable changes apply to the next outer emission.
An enabled hook with no callback slots still counts as an enabled subscription.
Manual destroy notifications run in the host immediate phase after current
promise jobs. The existing async-local-storage backend remains in use.

`EventEmitterAsyncResource` lazily composes the canonical emitter and resource
classes. Private weak maps own emitter/resource links and brand checks. The
resource's emitter link is initialized after its base resource constructor,
including init hooks. Dispatch resolves the current super emit hook and calls
it through the resource's public scope method. Argument prepending uses the
captured array primitive. Public resource getters and destruction methods
retain their Node method/getter shapes.

This covers explicit resource scopes and manual lifecycle notifications.
Promise hook emission, automatic GC destroy callbacks, fatal hook-callback
error handling, timer/network callback context propagation, and full internal
symbol/frame identity still require implementation and oracle evidence.

## Working directories and preloads

A successful `process.chdir` replaces the logical directory owner and clears
the successful `process.cwd` observation cache. A later successful cwd query
is cached until another chdir, matching Node even if the directory is removed
after that observation. An uncached query of a removed directory reports the
Node `ENOENT`/`uv_cwd` fields and diagnostic.

On Linux, the directory owner retains an `O_PATH | O_DIRECTORY` descriptor with
close-on-exec ownership. Child launch captures that owner after guest options
and environment hooks. Default launches inherit the retained directory;
relative cwd strings resolve there through kernel directory syscalls, including
after rename or unlink. Absolute cwd strings resolve directly. Only
directory syscalls run in the pre-exec callback. The descriptor remains owned
until launch completes. Live pathname queries use procfs when available, with
a cached-path fallback; other platforms retain the existing pathname path.

Eval/stdin module setup uses Node's `tryGetCwd` policy, falling back to the
executable directory when cwd cannot be queried. Absolute entry and preload
paths do not require a readable cwd pathname. `-r`, `--require`, and
`--require=...` preloads execute in order through the ordinary CommonJS loader
and cache before the entry. A preload is separate from the main entry.

POSIX `path.resolve` validates consumed arguments from right to left and stops
at an absolute prefix. Absolute resolutions do not read `process.cwd`; empty
and dot-only resolutions retain Node's early cwd shortcut and retry behavior
when a replaced cwd hook returns a relative path.

Child-process cwd options accept strings, file URLs, Buffer and Uint8Array
views. URL conversion and null-byte validation run before launch. Validated
byte views then use public string coercion, matching the child binding: Buffer
uses its string conversion, while an ordinary Uint8Array produces comma-separated
values. These options still use the same retained-directory launch owner.
URL branding currently shares the existing URL adapter; this does not establish
conformance when guests replace the global URL prototype.

Synchronous child output uses intrinsic byte views with the captured Buffer
prototype, independent of a replaced `Buffer.from`. The result owns one output
array; stdout and stderr are projections of its entries. Encoding conversion
calls the public Buffer `toString` hook and replaces those same entries.
Successful launches omit `error`; failed launches have pid zero, null output
and undefined stdout/stderr. Result property order matches the Node binding.

Process stdout/stderr writes pass ArrayBufferView bytes directly to OS
descriptors instead of converting through UTF-8. The existing embedding
`NodeOutputSink` remains text-only and uses its prior lossy text projection.
This does not establish complete Writable metadata, callbacks, string-encoding
options, backpressure or stream-error handling.

On Unix, synchronous child input and output share one nonblocking poll loop.
The configured stdio specifications own the OS descriptors and determine null
output slots. String input shares Buffer's codec using the original UTF-16
units; views use their raw bytes. Empty input closes the parent stdin endpoint.
Runner errors take precedence over the first pipe error. A timeout or buffer
overflow sends the selected signal, closes every parent endpoint and reaps the
child. A failing signal operation uses Node's SIGKILL fallback. The child's
actual exit status is preserved, including handlers that exit with a code and
children that exit before a grandchild-held pipe reaches the deadline.

Linux signal constants are read-only entries in a null-prototype record,
derived from the same OS table as captured process-kill constants and
synchronous child signal validation. Collection is implemented on Unix; non-Unix retains its previous
collection path. Signal selection and these additions are verified on Linux. Extra synchronous stdio channels,
wrap-based synchronous stdio, full option normalization and mutable internal
child-process callback routing still require conformance work.

## TLS material validation

Secure-context CRL and PKCS12 checks use native OpenSSL parsers. CRLs use
the PEM parser; arrays follow the guest iterator protocol and close on a
validation error. PKCS12 uses the BIO decoder and native password bytes,
including zero termination, so malformed input and incorrect passwords come
from the same parsing operations as Node. PFX arrays capture length, skip
holes and read each entry immediately before validation. Missing private keys
and certificates produce the corresponding coded operation errors. Parser
failures drain the OpenSSL error stack and expose the first reason.

This checkpoint validates material; the existing SecureContext object remains
a compatibility shell. It does not retain an SSL_CTX, configure trust stores
or establish TLS socket/handshake support. Provider-dependent unsupported
archives and the complete secure-context option surface need further work.

## Verification

Use the selected pinned Node oracle on `PATH` and build both sibling entry
binaries before testing child launches:

```sh
cargo build --profile iteration -p quench-node -p quench-node-test \
  --bin quench-node --bin run-parallel -j2
cargo test --profile iteration -p quench-node-test --test child_entry -j2
target/iteration/run-parallel --profile framework-core
```

Focused upstream checks use the selected pinned fixture directory:

```sh
target/iteration/run-parallel --all --parallel-dir "$NODE_PARALLEL_DIR" \
  --filter test-event --jobs 4 --timeout-secs 30 --results work/events.json
```

Complete sweeps use frozen sibling binaries and retain every case outcome,
binary/source revisions, inventory identity, deadlines and resource counters.
Their results belong in `evidence/`; work status belongs in Linear.
