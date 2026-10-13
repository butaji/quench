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

Common child options are copied into a null-prototype record through own keys,
per-key descriptor checks and original getter receivers. Common field rules
validate boolean flags, signed 32-bit uid/gid, shell/argv0 strings, timeouts,
kill signals and asynchronous serialization mode. Native option and file/NUL
validation replace the former bridge checks. Argument snapshots and the full
internal mutable child-process routing still need separate work.

Unix child credential/session configuration runs in pre-exec after directory
setup, with no guest calls. It clears supplementary groups opportunistically,
then applies gid and uid. Async launch errors preserve Node's synchronous
versus queued errno boundary. Sync errors retain the actual kernel errno;
code names and messages are projections, not reverse lookups. `getgroups`
queries the kernel and includes the effective gid when absent.

Synchronous command options preserve `execFileSync` shell choices and apply
`execSync` shell defaults in native code. Child disposal observes public killed
and kill hooks and returns undefined. Linux signal records are non-extensible
through the shared VM's canonical integrity operation. On Unix,
`node_shared_openssl` compares the loaded image owning SSL_CTX_new with the
image owning the host implementation; it does not infer linkage from fixture
names or a build environment. Non-Unix retains the previous linkage fact.

Child process handles keep separate open/closing/closed phases and one
reference flag. Public ref/unref methods observe the handle guard and method
lookup, including the repeated getter, and return undefined. Native handle
methods validate their receiver identity. Process exit starts closure;
close dispatch and handle closure are distinct transitions. Captured hasRef
remains observable during closing and returns undefined after release.
Referenced process handles, pipe demand/ownership and parent IPC channels
contribute independently to loop liveness. The current handle adapter covers
reference ownership; complete ProcessWrap construction/spawn/close methods and
internal channel ref-count semantics remain separate work.

## Local socket transport

On Unix, `net` path endpoints use OS Unix-domain listeners and streams. TCP
and local streams share the same native byte reads, queued write completions,
half-close operations and socket ownership. Accepted local sockets have no
fabricated IP peer address. Listener ownership unlinks a filesystem socket on
close; Linux abstract addresses use the kernel namespace and own no path.
Connect failures preserve OS errno, code, syscall and the requested path.
Listen failures enter the canonical next-tick queue, including bind conflicts
and overlong-path rejection. Endpoint overload selection is native; connect
and listen retain their different port/path precedence.

Oracle checks cover bytes exchanged with an external AF_UNIX process,
filesystem unlink, ordinary path overloads, ENOENT, asynchronous EADDRINUSE,
EINVAL for long paths, and Linux abstract close/rebind. Relative paths resolve
against the live working-directory path. Resolving a relative path to an
absolute pathname can still change the socket address length limit, and does
not preserve directory-fd semantics through concurrent renames. Server fd
adoption, the complete option/property-access surface, permissions flags,
socket reference ownership and Windows named pipes remain separate work.

## Listener reference ownership

`net.Server` ref/unref methods preserve the public `_unref` request and the
repeated `_handle` getter/method lookup. An unref request before listen applies
when the native listener is installed; changing an installed handle changes
the listener's single reference bit. Accepted sockets contribute independently.
The event loop derives net liveness from the transport, rather than counting
retained wrapper maps as additional references.

Native listeners transition from open to close-requested to closing, releasing
the OS endpoint at close and retaining handle reference queries through close
callback dispatch. The next native poll releases the handle metadata. Oracle
checks cover before/after-listen unref, ref restoration, public setter/getter
hooks, strict detached calls and close-phase hasRef observations. The exposed
handle adapter implements ref/unref/hasRef; complete TCP/Pipe wrap methods and
prototype metadata, fd adoption and socket reference behavior remain separate
work.

## Net option validation and coded error ownership

Socket constructor object-mode rejection and connect endpoint validation run in
Rust before transport effects. Missing endpoint arguments, invalid host types,
NUL bytes and invalid lookup function types preserve Node error messages and
original option getter/exception behavior. Numeric and local path overloads
continue through the existing endpoint owner.

Coded TypeError prototypes have one native owner, shared private brand, and
separate direct/validator prototype presentations. The lazy prototype cache
preserves observable identity across errors of the same presentation. The
validator presentation inherits its coded prototype and supplies its own
constructor getter. This does not establish complete Node stack-frame filtering.
Full option ordering and family attempt/race semantics remain separate work.

## Net lookup callback ownership

A connecting endpoint distinguishes an external lookup callback wait from an
active native connect request. Both retain one handle reference bit; the
external wait does not independently keep the loop alive. The canonical DNS
module supplies the default lookup function, including its public override;
custom lookup functions receive the family/hints/all options and an ordinary
strict callback with Node metadata.

Native callbacks preserve the original error object, lookup event order and
next-tick error delivery. Valid results retain their order within each family,
deduplicate destinations and alternate families by the first valid preference.
The same endpoint identifier and reference bit transfer to the native request,
so preconnect writes retain their original byte/completion owner. A callback
after destruction cannot start a new request. Native IP predicates preserve
original string-coercion exceptions/counts and scoped IPv6 syntax.

The current DNS adapter resolves synchronously and queues an immediate callback;
its resolver flags and failure fields remain incomplete. Native TCP attempts
remain sequential worker operations with the existing timeout. Exact family
staggering, attempt events, aggregate errors and
physical cancellation of an already started worker remain separate work. This
checkpoint covers ordinary callback errors, invalid array/IP/family results,
preconnect bytes, callback metadata and inactive-lookup process liveness; it
does not establish complete formatter/proxy, option or TCP/Pipe wrap behavior.

## TCP connect error ownership

TCP connect failures retain their OS error and concrete destination through the
worker channel and transport poll. The guest error projects actual errno into
Node code/syscall/address/port fields and the IPv4/IPv6 message, preserving the
observable field order. No error code is reconstructed from localized text.
The runtime's intrinsic Error owner supplies these host errors, including when
the guest replaces the global Error constructor.

Oracle checks reserve external ports, compare real IPv4/IPv6 ECONNREFUSED
fields/messages/prototypes with Node, and poison the global constructor. The
portable standard-library resolver still lacks complete getaddrinfo failure
codes; its existing ENOTFOUND classification remains separate from OS connect
errno. Logical timeout errors without a supplied OS errno do not invent one.
Family aggregation/staggering and complete local-address/connect options remain
separate work.

## Socket reference ownership

`net.Socket` ref/unref methods preserve the public `_handle` getter and method
lookups. Without a handle, the public `once` hook receives the public ref/unref
method for the connect event. Connecting, connected and accepted sockets carry
one transport reference bit; successful connection transfers that bit rather
than adding another wrapper reference. Pending connect/write requests and close
completion retain their independent work lifetime after an unref request. Socket
activity timeouts use the canonical native timer reference operation to avoid
keeping an otherwise unreferenced transport alive.

Oracle checks cover public hooks and exceptions, method metadata, unref before
and after connecting, accepted-socket unref, and ref restoration. The handle
adapter supports ref/unref/hasRef. Socket close callback timing and closed-handle
queries still differ from Node, and complete TCP/Pipe wrap methods, prototypes,
fd adoption and kernel keepalive/nodelay option hooks remain separate work.

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
