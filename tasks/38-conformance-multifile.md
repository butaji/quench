# Task 38: Support multi-file conformance cases with // @filename:

## Status: COMPLETED

### What was done (2026-06-30)

The `split_units()` function already existed in `conformance.rs`. Added comprehensive tests:

- `test_split_units_empty` — empty source returns single unit
- `test_split_units_single_file` — source without `@filename:` markers
- `test_split_units_with_filename_markers` — multi-file source with markers
- `test_split_units_with_default_prefix` — content before first marker

### How it works

```rust
pub fn split_units(source: &str, default_filename: &str) -> Vec<(String, String)>
```

Splits source on `// @filename:` markers. Everything before the first marker uses `default_filename`. Each `// @filename: <name>` starts a new unit.

Example:
```ts
// @filename: a.ts
export const x = 1;

// @filename: b.ts
import { x } from "./a";
console.log(x);
```

Returns: `[("a.ts", "export const x = 1;"), ("b.ts", "import { x } from \"./a\";\nconsole.log(x);")]`

### Files changed

- `crates/quench-runtime/tests/conformance.rs` — 4 new unit tests

### Remaining work

- Execute multi-file units together in one context (currently each unit would need to be evaluated in sequence)
- Handle `import`/`export` between units (requires module loader)
