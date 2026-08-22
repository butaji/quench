# diagnostics_channel

Implemented reachable Bun-compatible surface:

- `channel(name)` returns a process-local named `Channel`.
- `Channel#subscribe`, `unsubscribe`, `publish`, and `bindStore` are functional.
- `subscribe`, `unsubscribe`, and `hasSubscribers` convenience functions are exported.
- `require('diagnostics_channel')` and the resolver path load the implementation.

Unsupported diagnostics integration with native runtime internals remains intentionally absent.

## Definition of done (this slice)

Node-compatible behaviors implemented and locked by their related upstream
fixtures (all under `tests/node/test/parallel/`, verified genuine — no
skip-vacuous):

- `Channel#publish(message, ...rest)` invokes each subscriber as
  `subscriber(message, this.name)`; the channel name is always the 2nd
  argument and extra publish args are not forwarded.
  → `test-diagnostics-channel-pub-sub.js`, `test-diagnostics-channel-object-channel-pub-sub.js`
- `Channel#unsubscribe` returns a boolean (true when a subscriber was
  removed, false when absent).
  → `test-diagnostics-channel-pub-sub.js`, `test-diagnostics-channel-sync-unsubscribe.js`
- `subscribe(subscriber)` and `channel(name)` throw `ERR_INVALID_ARG_TYPE`
  on invalid argument types.
  → `test-diagnostics-channel-pub-sub.js`, `test-diagnostics-channel-symbol-named.js`
- `channel(name)` accepts Symbol names; the channel registry is a `Map`
  so Symbol keys stay distinct and `channel.name` preserves the Symbol.
  → `test-diagnostics-channel-symbol-named.js`
- `tracingChannel(nameOrChannels)` validates the argument type, and each
  present channel property must be a `Channel` instance.
  → `test-diagnostics-channel-tracing-channel-args-types.js`

Measured: `cargo run -p quench-node-test --bin run-parallel` → 261 passed,
0 failed, 261 total (up from 257 in this slice).
