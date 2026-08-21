# diagnostics_channel

Implemented reachable Bun-compatible surface:

- `channel(name)` returns a process-local named `Channel`.
- `Channel#subscribe`, `unsubscribe`, `publish`, and `bindStore` are functional.
- `subscribe`, `unsubscribe`, and `hasSubscribers` convenience functions are exported.
- `require('diagnostics_channel')` and the resolver path load the implementation.

Unsupported diagnostics integration with native runtime internals remains intentionally absent.
