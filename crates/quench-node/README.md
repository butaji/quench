# `quench-node`

`quench-node` is the Node.js compatibility host built on
[`quench-runtime`](../quench-runtime).

## Boundary rules

- This crate owns Node module registration, host capabilities, process
  boundaries, and Node-compatible errors and effects.
- The runtime owns JavaScript syntax and semantics and must not depend on this
  crate.
- The host must never detect benchmark names or source, scores, checksums, or
  another JavaScript engine.
- Every script follows the same host path and preserves Node-observable values,
  ordering, errors, exit status, and I/O.

## Event listener inspection

`events.listenerCount()` delegates callable public methods with the original
receiver, then recognizes EventTargets through Node's public constructor marker
`Symbol.for('nodejs.event_target')`. Native inspection reads the canonical
EventTarget listener records or the AbortSignal listener store; it does not
maintain a second count. Event names retain their key types during inspection.
The target's constructor and delegated-method getters remain observable.
AbortSignal inherits EventTarget's constructor and prototype chains. General
EventTarget capture/options handling remains incomplete.

## Child environments

Async and synchronous spawning share native environment normalization. Enumerable
string keys include inherited properties and respect nearer non-enumerable
shadows. Undefined values are omitted; other values use JavaScript's ordinary
string coercion, preserving getter/conversion errors. String keys and values with
null bytes produce Node-compatible argument errors before OS spawning. Windows
keys are sorted and deduplicated without regard to case; validation evidence is
currently from Linux.

## TCP reuse-port listeners

`net.Server.listen({ reusePort: true })` enables OS socket sharing before bind,
using the existing TCP listener lifetime and polling. The option requires the
boolean `true`, matching Node's flag selection. Linux validation covers IPv4,
IPv6, and another process binding the same address and port. FreeBSD selects its
load-balancing socket option. General listen error projection, Solaris reuse-port
support, and explicit backlog handling remain incomplete.

Process stdout writes flush the host's line buffer at each write boundary, making
bytes visible to another process before the writer exits or emits a newline.
The embedding's configured text sink continues to receive writes directly.

EventEmitter listener storage and inspection methods are native: `eventNames()` preserves string and
symbol keys, `listeners()` unwraps once wrappers, `rawListeners()` copies the
stored wrappers, and `listenerCount(type, listener)` filters by function identity.
Counts do not call an overridden public `listeners()` method. Removal and error
monitor bridges consume raw wrappers so inspection cannot disable once cleanup.
Callable construction, constructor metadata, listener counters, and capture
rejection policy are native. Thenable rejections use the shared next-tick queue
and preserve the original error and public rejection hook. Idle listener arrays mutate in place; active arrays copy before listener changes.
Nested emission counters restore on throws, duplicate callbacks remove in reverse
registration order, and bulk removal preserves notification order and its numeric
update rule. Once state changes before public removal and delegates registration
through public methods. Error/domain and stream-owner policy still uses the legacy
bridge, and malformed public listener records remain incompletely qualified.


Listener limits use one native default shared by EventEmitter's accessor and
`events.setMaxListeners()` with no targets. Emitters consult that default until
an instance override is assigned; EventTarget snapshots it at construction,
while AbortSignal starts at zero. Target limits and warning state use Node's
private, exported symbol keys. Numeric validation permits Infinity and rejects
negative values and NaN; strict assignments preserve runtime property errors.
Emitter registration derives leak warnings from the canonical listener array
and schedules the original Error object. `process.emitWarning()` validates
options natively and preserves supplied Error identity through delivery and
`throwDeprecation`. Stack-constructor options and stderr formatting remain
incomplete, as do EventTarget leak warnings and emitter capture-rejection policy.

Console's write boundary handles errors natively. The instance's `_ignoreErrors`
policy defaults to true and honors an explicit false value. An owned temporary
error listener protects synchronous writes; per-stream callbacks install a
listener for asynchronous errors. Cleanup runs even after a write throws.
RangeError with the engine's stack-overflow message propagates unchanged.
Constructor validation, method binding, grouping and full formatting remain open.

TCP read/write failures keep the original OS error until projection, deriving
Node's code, errno, syscall and message from that authority. Interrupted reads
retry; interrupted writes retry on the next transport poll. Socket destruction
latches its error outcome before guest handlers run, and automatic close
delivery projects `hadError` from that outcome. Linux reset behavior is checked
against an independent OS TCP peer. Connect/listen error projection and public
Socket emit interception remain incomplete.

Custom inspection dispatch is native and runs before depth cutoffs and function
or Error formatting. It preserves receiver, remaining depth, original inspector
identity, disabled-hook getters, and prototype filtering. The public util module
shares a canonical inspector; internal users retain the original function when
its public alias changes. Proxy unwrapping, multiline indentation, and general
inspection formatting remain incomplete. Event constructor phase constants use
Node's immutable enumerable descriptors.
