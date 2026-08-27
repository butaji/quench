# Compatibility rules

Node compatibility work is organized by semantic API families. A stage is a
planning boundary, not a progress claim or a fixture-specific workaround.

## Rules

- Use the checked-out Node tests and local Node CLI as behavioral oracles.
- Model reusable API facts once in the shared declaration/IR layer.
- Generate registration, wrappers, validation, and metadata from those facts.
- Handwrite only observable algorithms and edge adapters.
- Keep filesystem, clock, process, resolver, network, crypto, and output
  effects behind explicit host capabilities.
- Reuse value, event, stream, resource, and module state machines across APIs.
- Treat `Proven`, `Guarded`, and `Unknown` as distinct evidence states.
- Unknown behavior uses complete slow semantics or fails visibly; it never
  silently selects a fast path.
- A compatibility result compares values, descriptors, identity, ordering,
  errors, exit status, and externally visible effects with Node.
- Passing because a stub avoided an assertion is not evidence.
- The runtime remains a complete JavaScript VM; the Node host adds no alternate
  semantics and never dispatches on benchmark or fixture identity.

## Completion rule

Claim a stage complete only when its declared family is reproducible from a
clean checkout, has no unexplained failures or timeouts, and the full local
verification suite remains green. Remove temporary manifests, probes, and
duplicate representations when the stage is complete.

## Stage families

### 0. Measurement and runner truth

- Keep discovery, classification, and oracle comparison deterministic.

### 1. Runtime and value semantics

- Establish coercion, identity, descriptors, errors, and completion behavior first.

### 2. Core data APIs

- Reuse value facts for buffers, encodings, paths, URLs, and utility APIs.

### 3. Events, scheduling, and async context

- Model callbacks, timers, promises, and cancellation as explicit transitions.

### 4. Filesystem, VFS, and module loading

- Separate pure resolution facts from capability-backed resource effects.

### 5. Streams and backpressure

- Share one stream state machine across classic and web adapters.

### 6. Network and protocol families

- Keep parsing pure and socket, clock, and DNS effects at the host boundary.

### 7. Process, child processes, workers, and clusters

- Model handles, messages, exit status, and cleanup as bounded resources.

### 8. Crypto, security, and policy

- Declare algorithm, key, encoding, and provider facts once.

### 9. Observability and performance APIs

- Keep instrumentation optional, bounded, and semantically transparent.

### 10. Web and special modules

- Reuse existing value, event, stream, filesystem, and capability mechanisms.

### 11. Full-suite closure and reduction

- Remove temporary probes, duplicate wrappers, and redundant capability branches.
