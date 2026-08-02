# Make compatibility bootstrap initialization lazy

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
