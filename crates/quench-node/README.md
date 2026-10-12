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

EventEmitter inspection methods are native: `eventNames()` preserves string and
symbol keys, `listeners()` unwraps once wrappers, `rawListeners()` copies the
stored wrappers, and `listenerCount(type, listener)` filters by function identity.
Counts do not call an overridden public `listeners()` method. Removal and error
monitor bridges consume raw wrappers so inspection cannot disable once cleanup.
Constructor metadata, callable constructor behavior, and some mutation/removal
semantics remain incomplete.
