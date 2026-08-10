# Make compatibility bootstrap initialization lazy

> Contract: This task is part of broad Node 24 compatibility across Linux
> x86_64, Linux ARM64, macOS, and Windows. Native addons and Node-API are
> excluded. Use the statuses and release gates in
> [compatibility-contract.md](../docs/compatibility-contract.md).

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target.
See `docs/authoritative-test-sources.md` for the Node, LLRT, Deno, WPT, and
Test262 reference roles.

## Goal

Reduce eager JavaScript setup while retaining the same Node-visible APIs.

## Scope

- Identify bootstrap sections that are only needed after a module or feature is requested.
- Lazily initialize optional wrappers and compatibility modules.
- Do not remove behavior required by existing Node fixtures.
- Keep bootstrap declarations and exceptional behavior readable; generate
  repetitive setup instead of preserving duplicated wrapper source.

## Done when

- Focused startup and module-loading stages pass.
- All moved initialization remains observable at the same API boundary.

## Status

Crypto compatibility construction is lazy behind a public-compatible proxy and
covered by `tests/node-compat/stage-384/bootstrap-lazy-crypto.js`. Other
optional bootstrap sections remain in progress.
Deferred initialization timing is verified by
`tests/node-compat/stage-385/bootstrap-lazy-state.js`.
Stream export initialization timing is covered by
`tests/node-compat/stage-386/bootstrap-lazy-stream.js`.
URL module initialization timing is covered by
`tests/node-compat/stage-388/bootstrap-lazy-url.js`.
OS module initialization timing is covered by
`tests/node-compat/stage-389/bootstrap-lazy-os.js`.
Querystring module initialization timing is covered by
`tests/node-compat/stage-393/bootstrap-lazy-querystring.js`.
Querystring stringification preserves Node's `URIError` and `ERR_INVALID_URI`
contract for unpaired UTF-16 surrogates. This is covered by
`tests/node-compat/stage-489`.
The querystring `unescape` export is writable, matching Node's mutable module
surface. This is covered by `tests/node-compat/stage-491`.
Unicode surrogate pairs in `querystring.unescapeBuffer()` are now encoded as
one scalar value. The regression is covered by
`tests/node-compat/stage-490`.

The complete current lazy-bootstrap focused set (stages 384–386, 388–389,
393, and 489–491) passes locally. Optional wrappers outside these covered
module families remain open.

The network socket bootstrap was reduced below the size gate by moving the
TOS accessors into `network-socket-tail.js`; stage 2332 and the application
examples continue to pass after the mechanical split.

The stream-pair, pipeline, and public stream-export adapter tail is now
registered separately as `events-tail.js`; stream stages 370 and 372 and all
application examples pass after the split.

The `PassThrough` and base `Stream` compatibility adapters now live in
`events-stream-tail.js`, with stages 370, 372, and 453 passing after the
mechanical class-boundary split.

The `Transform` class and compatibility adapter now live in
`events-transform-tail.js`; stages 372, 373, and 453 continue to pass.

The bootstrap is now further decomposed into `crypto-tail.js`,
`module-surface-03-tail-02.js`, `network-blocklist.js`, `events-head.js`, and
`core-head.js`. These parts preserve declaration order while keeping optional
compatibility setup out of the primary registration sections; the focused
stream, network, and application gates continue to pass.

The `Duplex` class and compatibility adapter are now isolated in
`events-duplex-tail.js`; stages 370, 373, and 464 pass after the split.

Readable iterator methods now live in `events-readable-tail.js`; stages 370,
448, and 464 pass after the prototype-method extraction.

The final readable accessors and `_emitEnd` lifecycle method were moved into
the same tail. `events.js` now satisfies the 500-line bootstrap size gate;
stage 448 and all application examples pass.

The `Writable` class and compatibility adapter are now isolated in
`events-writable-tail.js`; stages 370, 455, and 466 pass after the split.

Additional ordered-tail splits now cover the network socket prototype methods,
the `net/promises` module, filesystem bigint-stat normalization, async stat
adapters, and the final core require-dispatch hook. Network, filesystem, HTTP,
and application regression stages remain green after these extractions.
