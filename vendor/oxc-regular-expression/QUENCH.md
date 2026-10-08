# Quench integration for OXC regular-expression parser

Upstream source: OXC `oxc_regular_expression` 0.153.0, crates.io archive
SHA-256 `369f33abce8a9a6a49c97a16579d8bb42331f1dd0ea5826412729ab94085c155`,
upstream revision `7f56ec9301e7327b4402c3010c87803b58b6ea30`, path
`crates/oxc_regular_expression`. The matching upstream MIT license is retained.

This parser is the workspace's single regular-expression grammar authority.
Its recursive parsing consumes Quench's shared `quench-stack` budget and
reports exhaustion with a stable diagnostic kind. `QUENCH.patch` records the
local stack integration against the published 0.153.0 crate source. Runtime
lowering and matching remain in `quench-regexp`.
