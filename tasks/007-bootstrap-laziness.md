# Make compatibility bootstrap initialization lazy

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
- Keep `bootstrap.js` readable and uncompressed.

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
