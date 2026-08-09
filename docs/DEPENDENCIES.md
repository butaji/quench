# Dependencies

Every dependency must replace more handwritten code than it adds or provide
specification behavior that cannot reasonably be hand-written. Add its row in
the same change that first uses it. Do not add crates speculatively.

| Crate | Purpose |
|---|---|
| `oxc` | Canonical parser, AST, and semantic scope/symbol data. |
| `oxc_ast_visit` | OXC AST traversal where a reducer query needs it. |
| `regress` | RegExp execution support. |
| `chrono` | Date arithmetic and conversion. |
| `num-bigint` | BigInt arithmetic. |
| `serde_json` | JSON parsing and serialization. |
| `urlencoding` | URI percent encoding/decoding. |
| `indexmap` | Ordered storage where ECMAScript order is observable. |
| `rustc-hash` | Internal non-observable hashing. |
| `phf` | Static generated lookup tables. |
| `tracing` | Explicit diagnostic instrumentation. |
| `anyhow`, `walkdir`, `tempfile`, `serial_test` | Test and runner support. |

`oxc` is the sole syntax and semantic frontend. A dependency that introduces a
second parser, syntax tree, type runtime, optimizer IR, or executor conflicts
with ADR 0005 unless that ADR is amended first.
