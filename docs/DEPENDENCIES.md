# Dependencies

Every dependency must replace more handwritten code than it adds or provide
specification behavior that cannot reasonably be hand-written. External LOC is
not free: measure enabled features, binary text, static data, compile time, and
runtime RSS. Keep every dependency behind a narrow canonical semantic adapter.
Add its row in the same change that first uses it. Do not add crates
speculatively.

## Current dependencies

| Crate | Purpose |
|---|---|
| `chrono` | Date arithmetic and conversion. |
| `num-bigint` | BigInt arithmetic. |
| `oxc` | Canonical parser, AST, and semantic scope/symbol data. |
| `regress` | RegExp execution support. |
| `serde_json` | Internal JSON serialization kernel behind JavaScript semantics. |
| `tempfile` | Temporary filesystem isolation in `quench-test262`. |

## Candidate dependencies

This table is an evaluation queue, not approval to add a crate. Add a candidate
only with the feature that needs it, after measuring the acceptance criteria
above, and move it to the current table in that same change.

| Candidate | Possible narrow use |
|---|---|
| `oxc_ast_visit` | OXC AST traversal where a reducer query needs it. |
| `regress` | RegExp execution support. |
| `urlencoding` | URI percent encoding/decoding. |
| `icu` / selected ICU4X crates | Modular ECMA-402 locale, calendar, formatting, collation, and segmentation data; use generated minimal data. |
| `indexmap` | Ordered storage where ECMAScript order is observable. |
| `rustc-hash` | Internal non-observable hashing. |
| `phf` | Static generated lookup tables. |
| `tracing` | Explicit diagnostic instrumentation. |
| `anyhow`, `walkdir`, `serial_test` | Test and runner support. |

`oxc` is the sole syntax and semantic frontend. A dependency that introduces a
second parser, syntax tree, type runtime, optimizer IR, or executor conflicts
with the repository doctrine unless that doctrine is amended first.
