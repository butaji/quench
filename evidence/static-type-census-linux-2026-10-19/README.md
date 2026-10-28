# Static type census: frameworks and the Node HTTP library

Linux x86_64 cloud guest. Runner: `quench-node` production build at `0de9005e`
(`target/production/quench-node census.cjs <files>`). Parser: acorn from
`work/web-upstream/node_modules`.

The census counts syntactic sites whose value type, shape or call target is decidable
before execution. The counts are static (sites), not dynamic (executions).

## Inputs
- `work/web-upstream/dist/node/{hono,fastify,express}/index.js`, as the bun-http-framework
  benchmark builds them. The prefixes of their hashes are in `inputs.sha256`.
- Node HTTP library, as quench-node loads it from `tests/node/lib`: `_http_server.js`, `_http_outgoing.js`,
  `_http_common.js`, `_http_incoming.js`, `events.js`,
  `internal/streams/{readable,writable}.js`, `net.js`, `buffer.js`.

## Outputs
- `frameworks.json` and `nodelib.json`: per-file counts.
- The census rules are in `census.cjs`. Name-based binding analysis is approximate: it is by
  identifier name, not by scope.

## Notes
- Bundlers rewrite `class X {}` to `var X = class {}`: hono has 18 such bindings. A static
  "bound once" analysis must therefore cover `var` bindings that are never reassigned,
  not only `const`.
- Static-key object literals are about 90% of all object literal sites:
  hono 113/153, fastify 1447/1810, express 2830/2925, Node HTTP library 203/230.
