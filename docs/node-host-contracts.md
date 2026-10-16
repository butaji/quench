# Native Node host contracts

Node API policy belongs to `quench-node`; JavaScript coercions, property access,
exceptions, roots and jobs use the shared VM. Existing class/function bridges
supply observable syntax and delegate policy to native operations.

## Node compatibility target

`process.version` and `process.versions.node` identify the bundled Node standard
library's compatibility target. The build derives this version from that
checkout's `src/node_version.h`; it is independent of Quench's engine version.
This version does not establish complete API parity. Implemented capability
flags and native dependency metadata have their own contracts.

## Captured JavaScript stacks

The Node host disables ECMAScript proper tail calls: strict caller frames remain
observable, as in Node. Core's default host policy and Wasm tail calls retain
their existing contracts. General and specialized dispatch use this same policy.

Captured errors and exposed CallSite objects trace the original callable and
receiver. Strict-frame visibility controls `getFunction` and `getThis`; function
debug names are derived when read. Method names inspect receiver descriptors
without executing accessors or Proxy traps, including inherited properties and
ambiguous enumerable aliases. CallSites share a realm-owned native prototype.
Constructor flags come from actual activation entry, including materialized
specialized frames. A pending `new.target` belongs to its callee, so reentrant
ordinary callbacks cannot inherit a constructor entry. Receiver type names use
the shared safe constructor-name operation; constructor/global receivers report
null, and Proxy receivers do not invoke traps.

Public function names and inferred debug names remain distinct. Variable
initializer inference follows result-producing sequence, conditional and logical
expressions without changing the public `.name`. A slotless shape transition
records when the native name-property state is reconfigured; reading the name
or inspecting its descriptor does not reconfigure it. Shape compaction preserves
creation names and the name/length descriptor transitions. Prototype setup and
named native-info lookup are separate lifecycle states. A named native name or
length read prepares the prototype chain; computed reads and Reflect.get retain
the setup view. Getter-free diagnostics derive native-name accessor visibility
from that state and the actual ordered descriptor position, including deletion
and restoration of length. The added metadata opcode requires
bytecode format 90; opcodes outside the dense fast table use general dispatch.
This covers those inference contexts, not every V8 debug-name inference rule.

CallSite `isEval` derives from the stored program's compilation origin, which
is independent of its binding environment. Full eval, expression eval and
Function-constructor bodies retain that origin across escaped closures and
collection. Ordinary functions called from eval and host-compiled scripts remain
source code, including host scripts with eval-like filenames. Captured CallSites
copy the classification before their program can be reclaimed. Complete eval/sourceURL/source-map metadata and the Node stack API still
require conformance work.

Function factory origins derive from the existing function kind and creation
realm. The realm owns its original constructors and intrinsic prototypes, so
changing public globals or prototype `constructor` properties cannot redirect
later factory allocations. Constructor debug names observe data properties and
preserve original names when accessors would otherwise execute. Native and bound
functions retain their distinct constructor-name behavior.

The optional retained host stack-policy callback receives the realm global,
original error and genuine captured CallSites. The Node host uses the configured
library's `prepareStackTraceCallback` and public `ErrorPrepareStackTrace`, so
internal overrides and user hooks follow the original algorithm. Formatted values
are cached after success; a thrown formatter leaves the capture available for a
later attempt. Native coded errors share the library's private Node error symbol.
UDP buffer-size errors construct the original `ERR_SOCKET_BUFFER_SIZE` class,
which owns its context getters and custom inspection behavior.

Eval CallSites retain the actual compiling caller's immutable source name and
position, including nested eval and Function-constructor origins. Capture reads
compiler/creation metadata and executes no public name getters. Unnamed eval
scripts expose an absent filename through CallSites and an empty script name
through detailed capture. Empty frame debug names use the eval display label.
Wide call anchors use the decoded opcode during source relocation.

Indirect global eval reuses live compiled-script identity across contexts;
execution supplies the target realm and allocates fresh values. Strict scripts
and tagged-template syntax are excluded according to the configured Node/V8
cache rules. Parser-owned syntax includes nested template sites. Entries use the
program store's existing reclamation lifecycle, validate the program identity
on lookup and disappear when their program is reclaimed or the store resets.
Complete V8 cache ageing, contextual direct-eval identity, sourceURL/source-map
metadata, complete prototype-cache transitions, raw UTF16 Function-constructor
source and compatibility-normalized parameter positions remain open. Function-constructor public names are initialized as
creation data: public anonymous is distinct from the empty default source/debug
name. Stack names and native constructor display share this distinction across
all four constructor kinds without invoking public name getters.

Function-constructor compilation and public source text share a kind-owned
canonical renderer. Public source preserves parameter/body whitespace, and
compiled source retains the expression wrapper that Node includes in first-line
parameter columns. Body lines begin after the canonical header newline. OXC
validates the generated identifier and parameter boundary; only the synthetic
outer identifier is removed before semantic binding, so anonymous is not a
self binding. Nested names and real parameter bindings remain intact.

## Parsed eval execution

Direct and indirect eval execute the existing OXC-compiled Eval goal, retaining
the original source and statement completion values. Host Script evaluation
uses the Script goal, including single expressions. Neither path interprets
source substrings or inserts an expression-return wrapper. Binding scopes,
strictness, early errors and captured environments share the compiler and VM
operations used by other code. A lone regexp expression uses OXC's literal span
and flags with the original UTF-16 pattern units, preserving lone surrogates.
Complete eval/sourceURL metadata, synthetic diagnostic filenames and
Function-constructor wrapper positions still need separate conformance work.

## Detailed source stack capture

The public `util.getCallSites` function comes from the unmodified configured
Node `util.js`; its native binding captures actual VM source activations,
independently of `Error.prepareStackTrace` and `Error.stackTraceLimit`. Results
are null-prototype records with copied UTF-16 function names, compiled-source
identities, mapped filenames, positions and the column alias. Native helper
frames are excluded. Error CallSites and detailed capture share the source-frame
projection; host filename mapping stays an explicit mutable host boundary.

The frame cursor explicitly distinguishes an executing guest-capable operation
from a saved call continuation. Source inspection uses that state; GC root-map
and lexical-binding projections keep their existing owners. General calls,
materialized lane calls and restored activations preserve the distinction.

Call positions use OXC expression/comment spans to identify method names and
actual argument parentheses, including optional/private calls, returned
functions, comments and line breaks. Property reads and writes have distinct
operation positions, including assignments and updates. Source lines recognize
all JavaScript line terminators and columns count UTF-16 units. Fused
field sites retain separate read/read/write source coordinates. A scoped active
component selects the correct location while executing a getter or setter and
restores the previous source context after success or failure. Bytecode format 91
serializes these locations and rejects invalid coordinates. Complete eval/sourceURL,
source-map and Node host-stack conformance remains open. Source IDs denote actual compiled units; missing host frames are not
fabricated.

## Boxed primitive identity

Primitive wrappers carry their actual value in an object-internal payload.
Builtin brands, primitive methods, constructor display, structured cloning and
JSON unboxing derive from that payload. Public properties cannot manufacture or
replace wrapper data, and Proxy wrappers do not inherit the target's brand.
The payload follows the shared object-extras GC root visitor and releases when
the wrapper dies. Intrinsic Boolean, Number and String prototypes have their
specified primitive data; Symbol and BigInt prototypes do not.

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

A distribution with static OpenSSL includes the matching native OpenSSL CLI as
`openssl-cli` beside `quench-node`. Freeze that tool with the sibling engine and
runner, and retain its hash and `openssl-cli version` output. Consumers of
`node_shared_openssl` select this adjacent CLI for static builds. Copying only the
engine and runner omits a real package dependency and causes CLI-backed crypto
and TLS checks to skip. CLI subprocesses execute OpenSSL; JavaScript still runs
in Quench. The frozen Linux qualification bundle copies `/usr/bin/openssl` from
the same system installation used for the configured OpenSSL headers/archives.

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

Coded TypeError and RangeError prototypes have one native owner, shared private brand, and
separate direct/validator prototype presentations. The lazy prototype cache
preserves observable identity across errors of the same presentation. The
validator presentation inherits its coded prototype and supplies its own
constructor getter. This does not establish complete Node stack-frame filtering.
The family-attempt timeout has a 500 ms invocation default and a shared native
validator for the public setter and TCP connect options. The validator accepts
positive signed 32-bit integers, clamps values below 10 ms, and preserves coded
TypeError/RangeError prototypes, messages, and original getter exceptions.
The CLI override preserves its value and selects the last supplied value,
independently of the setter's 10 ms clamp. This covers configuration
validation; the existing sequential connection worker does not yet implement
Node's per-attempt timeout scheduling.
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
adapter supports ref/unref/hasRef. Complete TCP/Pipe wrap methods, prototypes, fd adoption and kernel
keepalive/nodelay option hooks remain separate work.

## Stream and socket close completion

The native stream adapter owns `emitClose` option interpretation and close
emission policy; the existing guest stream state machine forwards to that
operation. Readable/Writable/Duplex destruction still emits its original error
when close emission is disabled and completes the closed/closeEmitted views.

Sockets disable generic stream close emission. Their OS endpoint closes when
requested; the named transport close phases retain the captured handle query
until completion. The guest socket root transfers to the close-completion
projection and then into its scheduled callback, with no additional wrapper-map
liveness reference. Completion releases native reference metadata before the
public close callback, so a captured socket handle reports false. Public manual
close events do not drive this transition. An unconnected socket completes its
stream callback before queuing the no-argument close event.

Oracle checks cover ticks before physical close, captured reference release,
ref after close, original error/close arguments, manual close events, unconnected
sockets and emitClose false for all three stream kinds. Full handle-wrap methods,
prototype/brand behavior and complete generic destruction/property ordering
remain separate work.

## Native TLS configurations

`SecureContext` owns a real OpenSSL `SSL_CTX` builder. Its VM inner object has a
weak host identity and explicit Allocated/Initialized phases. Unreachable
identities release native configurations at job checkpoints. Contexts add no
referenced event-loop work. Synchronous exit listeners retain access to held
contexts; native entries and private builtin roots are released after those
listeners, including error exits. OpenSSL SSL objects retain their own context
references when transport support is added.

Rust implements protocol selection, version/options validation, configuration
snapshots and native setters. Checked JavaScript functions/classes forward
calls and preserve callable construction and method metadata. TLS captures the
canonical private crypto-binding constructor when its module loads, preserving
earlier replacements and ignoring later replacements as Node does. Engine
method availability and repeated getter effects follow the actual inner object.
Original public
method/property hooks retain their ordering and exceptions. Native crypto
string arguments use Node's UTF-8 C-string prefix, including embedded nul
handling; the existing rooted VM bitwise operation owns integer coercion.

Certificates, keys, CA roots, CRLs and PKCS12 material are retained by the native
configuration rather than discarded after validation. Bundled public trust
roots come from the pinned Node product source; their provenance and license
sit beside the PEM data. CRL arrays use the original iterator and close on
errors. CA/certificate/PFX arrays capture length and skip holes; key arrays read
length on each iteration. PKCS12 uses the BIO decoder and password bytes with
C-string termination. Native certificate outputs use intrinsic typed bytes
and the original Buffer prototype, independently of public Buffer aliases.

Crypto and TLS share one native engine loader and DEP0183 warning owner.
The loader uses OpenSSL's structural/functional references and dynamic loading
fallback. TLS private-key engine ownership follows the SSL_CTX's final native
reference through typed ex-data. Client engines are retained by SSL_CTX.
Public engine methods follow the linked library's actual build capability.
Crypto setters share OpenSSL's decorated errors; engine searches retain Node's
plain error plus reversed remaining error stack. PKCS12 retains its distinct
reason-only parser presentation.

Cipher discovery enumerates the linked OpenSSL provider's SSL cipher stack and
Node's named TLS 1.3 suites. The logical process caches lowercase, deduplicated,
sorted names, and each call returns a fresh intrinsic array. Public function
shape and the complete list are compared with the pinned Node oracle.

Certificate hostname authorization shares one Rust matcher. It applies DNS SAN
precedence, common-name fallback, IP canonicalization, IDNA normalization,
restricted wildcards, and quoted SAN parsing through the shared JSON parser.
Original certificate getters precede hostname coercion with the default
primitive hint. Public string/array methods and net.isIP remain observable;
errors retain the original certificate identity, reason, and hostname.
Malformed quoted SANs use the realm's intrinsic SyntaxError. Node comparisons
cover values, errors, getter order, boxed names, and custom SAN method hooks.

Native TLS client/server streams now use OpenSSL inside the canonical TCP/Unix
owner. Raw stream ownership moves once into SslStream. The transport preserves
one write queue, offset, completion list, reference bit, and close lifecycle.
Original connection callbacks finish before any stream IO. Handshake completion
waits for authorization before application IO; client verification invokes the
original hostname callback after native chain verification.

Connection and data delivery release their transport gates in native completion
operations, including when the original callback throws. Each read delivers its
bytes before another physical read, so callbacks can destroy or otherwise change
the socket before more input is consumed. Resource reporting derives reference
state from the transport owner, including accepted sockets inside connection
callbacks and handles awaiting native close completion. The general scheduler
does not infer socket lifecycle transitions from callback argument positions.

SSL writes retain their retry extent while queued data appends. Readiness returns
to the event loop; native SSL errors keep their typed payload and shared error
projection. TLS close_notify becomes readable EOF. Duplex data/end/finish owns
destruction so coalesced payload and EOF drain before close. HTTP endpoints keep
their explicit EOF policy. Temporary native SSL error stacks are consumed by
queued guest callbacks and cleared at cleanup.

TLSSocket construction owns a real SSL session before a physical connection
exists. A weak VM identity owns the pending session; the SSL object transfers
once into the canonical stream when connected. Direct clients explicitly start
the handshake with `_start`; direct servers start automatically. Direct sockets
emit `secure` without helper authorization or `secureConnect`, while client and
server helpers retain their distinct authorization policies. Native weak-owner
pruning and exit cleanup release unused pending sessions.

Wrapping an existing socket transfers its unread buffered bytes to that same
SSL input owner at next-tick initialization. Socket byte accounting observes
native data delivery without adding a data listener that forces readable flow.
Physical read EOF pushes null and reads zero, matching Node's stream-base path.
Duplex half-close follows readable-end emission and defers its finalizer without
adding an observable event listener. Physical read EOF stops a referenced socket
from keeping the event loop alive once write and shutdown requests finish;
incoming EOF does not independently shut down writes. Empty-error native SSL
syscall EOF becomes stream EOF after handshake; real syscall and SSL failures
retain their error projection.

Independent Node peers verify encryption in both directions, exact binary data,
class/protocol state, trusted CA/hostname success, untrusted-chain rejection,
and hostname-mismatch fields. Peer names, RSA key data, fingerprints, validity,
and DER come from actual X509 material. API qualification uses its established
default library-root environment; exporting a configured root exposes an
existing 300 ms child-startup/signal timing failure in both the prior published
binary and this source. Those failed observations remain retained. Fixture runs
continue using the pinned configured library root and unchanged 30 s deadline.

This stream foundation remains incomplete: HTTPS integration, direct TLSSocket
lifecycle details, original handle query forwarding, handshake timeout policy,
ALPN/SNI callbacks, sessions/ticket keys/PSK, EC and complete legacy certificate
metadata/chains, trust CLI/environment/permission policy, and remaining option,
reflection, error, hostile-method-return and UTF-16 details are open. Focused
TLS/TCP gains do not establish a new full-suite total or a performance claim.

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

## Lazy web globals

`URL`/`URLSearchParams`, `TextDecoder`/`TextEncoder`, the DOM event family
(`DOMException`, `AbortSignal`, `AbortController`, `Event`, `EventTarget`,
`MessageEvent`, `MessagePort`, `MessageChannel`), the web-stream family and
the web-API group (`Blob`, `Headers`, `Request`, `Response`) are lazy data
properties of the global object, as in Node, and the runtime's `Temporal` is one
too. A group's polyfill runs once, under the startup intrinsics, when script
first reads one of its globals or asks for its descriptor; until then each name
sits at its bootstrap position. Every observation is a writable, non-enumerable,
configurable data property: `Object.getOwnPropertyDescriptor`, partial
`Object.defineProperty`, `freeze` and `seal` materialize the group first, while
`Object.keys`, `hasOwnProperty` and enumeration read only the attributes.
Assignment or deletion before materialization needs no value; the group's later
initialization keeps the script's value or absence. A polyfill runs as a block,
so its declarations never become global lexical bindings.

TextDecoder label resolution and complete-buffer Unicode/single-byte decoding
use Rust `encoding_rs`. The Unicode BOM policy applies only to UTF-8 and UTF-16;
legacy encodings retain a decoded U+FEFF. Legacy multibyte mappings and error
consumption follow ICU's converter C API, matching Node's backend. Cargo detects
`icu-uc` through `pkg-config` and links the reported libraries; an ICU development
package (for example, Debian's `libicu-dev`) enables this backend. Cross builds
must provide target-compatible pkg-config metadata and libraries. Without ICU,
Unicode and single-byte codecs remain available and unsupported multibyte labels
throw `ERR_ENCODING_NOT_SUPPORTED`.

Qualification must record both engines' ICU versions: converter tables can vary
between releases. Complete-buffer comparisons do not qualify streaming decode,
all input coercions, detached/shared buffers, or constructor metadata.

## Buffer string construction byte projection

`Buffer.from(string, encoding)` obtains encoded bytes from the shared Rust
codec and passes an intrinsic Uint8Array to the Buffer constructor. The temporary
view owns fixed backing storage and does not invoke guest constructors,
iterators, or indexed hooks. The Buffer result retains its ordinary brand and
independent contents across GC.

Other private encoder callers currently consume a number-array projection from
the same codec. Their existing byte-length, write, and fill operations keep that
projection until their method and property hooks have their own native contract.
The oracle covers original byte contents, UTF-8 replacement, hex truncation,
base64, GC, constructor/iterator poisoning, typed length getters, and subarray
hooks. Complete Buffer pooling, argument policy, and remaining method behavior
are separate work.

## Fatal promise rejection diagnostics

Throw and strict rejection modes share native exception routing. When exception
handlers decline the rejection, the original exception remains rooted through
fatal formatting, then its owned wrapper root is released. The fatal output
uses the existing uncaught-error formatter and preserves the original Error
stack. Primitive rejection wrappers use the intrinsic Error constructor,
independently of guest replacement of the global constructor, and install code
before name in the same field order as Node's wrapper.

This covers fatal diagnostics and the tested primitive wrapper fields; complete
wrapper subclass prototypes and remaining Error-like/proxy classification and
formatter behavior require separate qualification.

Net Server construction rejects non-nullish primitive options through the
shared direct coded argument TypeError owner before registering a connection
listener. Objects and listener functions remain accepted. Full Server option
copying, getter ordering, and configuration policy are separate work.

## Native Linux filesystem event watches

`fs.watch` on Linux uses one native nonblocking CLOEXEC inotify descriptor,
with libuv's event masks and event/name projection. Kernel watch groups retain
ordered subscriber identities and the first path's basename. Callback delivery
checks live ownership between subscribers and kernel records, so closure during
a callback cannot deliver into a stopped watcher.

Each handle has one reference bit and a named unstarted/watching/stopped
lifecycle. An active referenced watch holds the loop; a stopped handle preserves
its captured reference metadata without holding the loop. Weak handle metadata
is pruned by the shared collector. Watcher roots transfer out of native ownership
on stop; public close queues its event through the canonical next-tick queue.
Abort listeners are detached during the close event. Cleanup and replacement VM
bootstrap close the native descriptor and discard the prior watch state.

Filename encodings use the shared Rust Buffer codec. Buffer filenames are
intrinsic byte views; ordinary callbacks and ignore predicates preserve their
original exceptions and roots across GC. Option copying follows inherited
enumerable string properties and nonenumerable shadowing. Invalid encoding and
ignore values use coded errors with the tested messages and field order.
Function, RegExp, and simple wildcard ignore patterns are supported. Pattern
sources are resolved in Rust at creation and run through captured intrinsic
RegExp operations; RegExp instances retain their original lastIndex behavior.

Mounted virtual paths retain their existing VFS routing. On other hosts this
checkpoint preserves the prior fallback. Advanced glob semantics and complete
handle-wrap/proxy/error details remain separate work. The internal test uv
binding projects errno constants from the same platform table as
util.getSystemErrorName.

## Native filesystem stat polling

`fs.watchFile` keeps an absolute lexical path cache separate from active polling
ownership. Repeated subscriptions return the cached watcher and increment its
listener reference limit. Public stop releases the polling root and queues one
`stop` event through the shared next-tick queue; it does not remove the path
cache. `unwatchFile` removes that cache entry when no change listeners remain.
Reference counts control loop lifetime independently of cache size. The initial
stat request remains work even when the watcher is unreferenced, matching Node;
later unreferenced polls run only while other referenced work holds the loop.

Polling distinguishes initial, present, and missing states. The initial existing
file establishes a baseline, an initial missing file reports zero stats, and
error transitions preserve the most recent successful metadata. The comparison
uses libuv's stat fields rather than atime-only changes. Zero interval means one
millisecond; interval validation uses the unsigned 32-bit stat-poll domain,
independently of ordinary Timeout coercion. Options are copied from enumerable
own properties, including Symbols, before listener validation.

One exact integer metadata snapshot projects the ordinary Number stat view and
the watch's bigint view. The latter constructs integer primitives directly,
preserving nanosecond precision. Stat presentation owns constructor facades,
mode methods, lazy dates, and partial timestamp Symbols; date rounding calls
the shared VM operation. Loaded internal fs utilities retain their exported
BigIntStats identity. The tested contract includes constructor reflection,
original coercion/callback exceptions, GC, stop ordering, missing/created/deleted
files, and reference lifecycle. Complete internal handle and Symbol mutation
behavior, Temporal fields, all ordinary stat prototypes and bigint decorators,
complete iterator queue and stat/error policy remain separate work.

## Native recursive watch composition

On Linux, recursive watches compose the canonical public per-path `fs.watch`,
`statSync`, and `readdirSync` operations. Original overrides and exceptions remain
observable. One private class-data Map owns each path's stats, child watcher,
and symbolic-link classification. Callback cycles stay in the VM heap and remain
visible to its collector; the existing native child watchers own kernel lifetime.
Directory discovery, deletion, subtree removal, and relative names follow that
ownership. Ignored paths are filtered before child subscriptions are created.
Globstar directory prefixes may match zero path segments through the shared Rust
glob compiler.

The recursive class has its own public method contract: `close` emits synchronously
and repeated closure is inert. The pinned Node implementation's `ref` and `unref`
leave ordinary stat records unchanged and return undefined. Its async iterator
uses the native Promise and event operations, returns the tested null-prototype
results, and preserves abort causes. Recursive `fs.promises.watch` forwards through
the shared async-generator protocol to that iterator. Complete queue/overflow policy
and all private handle and
reflection details remain open.

The internal event binding exports the same event constructor as the public native
watcher. Internal URL conversion has its own cached namespace, preserves ordinary
input identity, and uses the URL state WeakMap as its brand authority. It does not
add the private helper to the public URL namespace. Callback `fs.rm` shares the
existing remove operation and recursive/force option policy, snapshots those
options once, and delivers completion through the canonical next-tick queue;
complete retry and option/error policy remains separate work.

## Native promise-watch queues and module names

Nonrecursive promise watches use a direct handle-callback projection of the same
native kernel subscription, reference, close, prune, and cleanup owner as public
watches. Public EventEmitter dispatch remains the public watcher projection.
One private heap FIFO stores values and overflow errors while its async-generator
consumer is suspended. Pending resolvers, option snapshots, abort hooks, and the
watching/closed state remain in that GC-visible private data. Native callbacks use
captured array/Promise operations, shared ignore matching, and existing abort
propagation-resistance options. Error completion closes the handle and detaches
abort hooks. Abort errors preserve original causes and tested descriptor/constructor
shape through one shared watch-error operation.

The tested surface includes lazy validation, burst buffering, abort before/after
startup, stopped propagation, and overflow error closure. Broader queue cancellation,
full private binding/start metadata, complete subclass/proxy/error reflection, and
all watch scheduling details remain separate work.

CommonJS named export recognition uses OXC syntax and the Node-matched static
object prefix/direct-assignment rules. Accessor or nested-value stopping points
preserve names discovered earlier; strings/comments and compound assignments are
not export assignments. The generated interop module includes the module.exports
alias and supports quoted export names. Full reexport/defineProperty detection and
file-URL dynamic import resolution remain separate work. Module namespace key
projection belongs to the shared runtime: string names sort by UTF-16 units,
without ordinary array-index priority; symbols follow the string names.

### Module URL inputs and search paths

The Rust `createRequire` adapter follows the pinned Node URL-like property checks in order (`href`, `protocol`, `auth`, `path`), then ordinary string coercion and the canonical file-URL conversion operation. Recognition getter errors escape unchanged; conversion errors become `ERR_INVALID_ARG_VALUE` for the original input. That error projection uses the captured internal inspector and caps the inspected value at 128 UTF-16 code units. A trailing directory separator adds the Node proxy filename before creating the ordinary CJS require record.

The `_nodeModulePaths` operation calls the existing public path resolver, preserving its receiver, argument, overrides and errors. Ancestor `node_modules` directories have one Rust projection shared with ordinary CJS module records. Thin checked function facades preserve ordinary constructor calls and function metadata while Rust owns their policy. Three Node comparisons cover these inputs, original hooks, constructor calls, error presentation and cwd behavior. URL query metadata, full Module construction/resolution hooks and global lookup paths remain open.

`pathToFileURL` uses the same public POSIX or Win32 resolver as Node, including guest cwd and original resolver hooks, and restores an original trailing separator. Its former fixture-relative remapping is removed. Shared path argument errors derive their received-value description from the existing Node utility.

## Linux OS facts and canonical policy

On Linux, the configured, unmodified `lib/os.js` owns public OS exports,
function reflection, primitive hooks, descriptors, validation and error classes.
Private OS and credentials binding objects have one retained identity in the
shared module cache. Rust supplies kernel, environment and account facts;
public mutable Buffer constructors are not used to construct native results.
Other platforms retain their existing adapter.

Temporary-directory selection skips empty TMPDIR, TMP and TEMP values, falls
back to `/tmp`, and removes exactly one trailing slash except at the root.
Home-directory selection observes HOME, including an empty value, then falls
back to the reentrant effective-user account lookup. Account fields come from
that record independently of HOME, and native encodings preserve their bytes.

Available parallelism combines scheduler affinity with libuv's cgroup CPU
quota rules. Load averages and fractional uptime use the kernel's proc data
with the corresponding syscall fallbacks. Interface addresses follow the
getifaddrs inventory, masks, MAC addresses, flags and IPv6 scope identifiers;
Node's guest policy derives public objects and CIDR strings.

Priority requests mutate the requested kernel process. PID zero maps to the
CLI process even though the logical main guest executes on a large-stack Rust
worker. Successful self-priority changes also update that worker so future
guest children inherit the logical process priority. The requested process
operation determines the public result and original Node policy constructs
system errors from the native context. This does not claim Node worker-thread
semantics or complete Node API parity.

## Canonical inspection and native snapshots

The configured Node `internal/util/inspect.js` owns public `inspect`, `format`
and `formatWithOptions` identities and internal formatting. Its initialization
captures private state using intrinsic methods, so it runs on first use inside
the startup intrinsics (`node_library_module`), which put back the original
methods however guest code has replaced or frozen them, nonconfigurable
replacements included, and restore the guest's afterward. A script that never
formats loads none of it. Native
key, type and symbol bindings have independent cache ownership, so they do not
recursively require public formatter construction. The partial duplicate
renderers and their color-table copies are removed.

Core snapshots expose Proxy target/handler pairs and collection entries without
traps, public methods or cursor advancement. Revoked proxies preserve nulls.
Collection snapshots read the live Map/Set slots and current iterator cursor,
including deletion/clear transitions; scoped roots keep entries alive across
native reentry and collection. Node policy derives display strings from them.
The native class fallback reads builtin slots and data tags without public
constructor or tag getters. Original user-constructor provenance after mutation
and exact Node stack rendering remain separate compatibility gaps.

## Private data fields

Private data lives with its receiver, keyed by the actual declaring home and
logical name. A private element retains both that home and its data through
collection. Different creations of one class expression can stamp the same
object without sharing values. Initialization checks duplicates before changing
the stored value; private data and method brands do not consult public
extensibility or Proxy traps, including revoked Proxy receivers.

The atomic private-data definition operation is serialized in bytecode format
92. Round-trip execution and generic/specialized collection tests cover the
representation. Eval-capable methods retain their synthetic class-home capture
in dynamic-name metadata, even when private syntax appears only inside eval's
string. Bytecode format 93 adds lexical private-name bindings that retain each
name's actual declaring home independently of the method's super home. Static
and instance names, escaped ordinary closures, and outer/shadowed class names
use that binding metadata. Ordinary-function eval uses OXC's ordinary-function
grammar inside a private-name parsing context, and the extracted body must match
the generated source boundaries. Complete private-environment contexts and the
ordinary NUL string-key namespace remain separate conformance work.

Private membership follows initialization: prototypes acquire no brand merely
from a declaration, static data fields remain absent until initialization, and
static private methods/accessors precede static fields. Auto-accessor backing
identities retain their own lexical declaring-home binding, including public
instance auto-accessors whose backing name is not visible to eval.

Static blocks retain the actual class home for `super` and execute in strict
mode. The compiler's class-initializer context is shared by field initializers,
static blocks and their arrows: eval forbids lexical `arguments`, preserves
`this`, and supplies undefined `new.target`. Ordinary nested functions introduce
their own context. The existing serialized initializer flag represents this
shared context; static blocks keep distinct variable scopes.
