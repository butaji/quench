# Authoritative test sources

These sources define how quench-node measures Node 24 application
compatibility and where implementation patterns come from.

## 1. Node.js test suite — primary oracle

- Repository: [nodejs/node/test](https://github.com/nodejs/node/tree/main/test)
- Vendored test directory reference:
  [denoland/node_test](https://github.com/denoland/node_test)

The initial compatibility manifest prioritizes:

- `test/parallel/` — built-in module and runtime behavior.
- `test/es-module/` — ESM loading, exports, and package `type` behavior.
- `test/common/` and `test/fixtures/` — shared test infrastructure.

`sequential/`, `message/`, and internet-facing tests are subsequent scopes.
Addons and Node-API suites remain outside the initial application gate.

The local Node suite is already tracked as `tests/node`; do not add a second
vendored copy under `tests/node-compat/suite`.

## 2. LLRT — closest architecture reference

- Repository: [awslabs/llrt](https://github.com/awslabs/llrt)

LLRT is a QuickJS/Rust runtime reference for separating JavaScript semantics
from Rust and for documenting supported, partial, and unavailable APIs. Its
unit, end-to-end, and WPT test organization informs quench-node structure; it
does not define quench-node's compatibility target.

## 3. Deno node compatibility — foreign-runtime runner reference

- Runner: [denoland/deno/tests/node_compat](https://github.com/denoland/deno/tree/main/tests/node_compat)

Deno's manifest and runner are references for running Node tests in another
runtime, recording expected results, and documenting skip reasons. Quench-node
uses the same principle through its versioned JSONC manifest, while retaining
its existing Rust runner and `tests/node` submodule.

## 4. Web Platform Tests — web APIs

- Repository: [web-platform-tests/wpt](https://github.com/web-platform-tests/wpt)

Relevant suites include `url/`, `fetch/`, `encoding/`, `WebCryptoAPI/`,
`streams/`, and `dom/abort/`. WPT supplements Node tests for standards-based
globals such as URL, fetch, encoders, streams, WebCrypto, and abort signals.

## 5. Test262 — ECMAScript baseline

- Repository: [tc39/test262](https://github.com/tc39/test262)

Test262 is a secondary baseline for QuickJS/rquickjs language conformance. It
becomes a direct regression gate if quench-node changes engine behavior.

## Implementation order

1. Node `assert`, `buffer`, `fs`, and `path` fixtures.
2. Node ESM and module-loading fixtures.
3. Node timers, streams, networking, and HTTP fixtures, plus relevant WPT.
4. Node crypto and zlib fixtures, plus WPT WebCrypto.

The complete runtime contract, release criteria, and application gates are in
[ADR 0001](adr/0001-node-24-application-runtime.md).
