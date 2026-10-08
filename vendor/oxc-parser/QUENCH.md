# Quench integration for OXC Parser

Upstream source: OXC `oxc_parser` 0.153.0, crates.io archive SHA-256
`71ce9e6d646d84b30f8eb4e1a1b50c69e66f4ef08066c721a3b82a451326bad4`,
upstream revision `7f56ec9301e7327b4402c3010c87803b58b6ea30`, path
`crates/oxc_parser`. The matching upstream MIT license is retained.

The local patch connects recursive parsing to Quench's shared `quench-stack`
budget, preserves exhaustion as structured parser state, supports the
runtime's Annex B call-assignment-target lowering option, and preserves sloppy
`let:` label parsing. OXC remains the sole JavaScript and TypeScript parser.
`QUENCH.patch` records these changes against the published 0.153.0 crate
source.
