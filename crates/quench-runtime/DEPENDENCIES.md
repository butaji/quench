# Dependencies

Rationale for every direct dependency in `crates/quench-runtime/Cargo.toml`.
Each entry explains why the crate was chosen, what it replaces (and why), and
how it interacts with the rest of the runtime.

When adding a dependency, update this file in the same diff.

## Parser — `oxc`

- **Crate:** `oxc = "0.47"` (umbrella re-exporting `oxc_parser`, `oxc_ast`,
  `oxc_allocator`).
- **Why:** Official OXC parser. Parses full ECMAScript + TypeScript + JSX,
  returns arena-allocated AST. ~20K LOC saved vs. hand-written parser.
- **Used by:** `src/parser.rs`, `src/lower/`.

### Arena lifetime

OXC requires an `oxc_allocator::Allocator`. The AST nodes are
`oxc_allocator::Box<'_, T>` whose lifetimes are tied to the arena. The
runtime keeps the allocator alive for the duration of a script's parse and
lower passes; the interpreter then operates on lowered values, not the OXC
AST directly.

### Why not `swc`?

Too complex — 20+ sub-crates. ~2K LOC of crate-graph glue before any
parsing work begins. `oxc` is the minimal parser.

### Why not `gc-arena` for the arena?

`gc-arena` is used by Ruffle, but requires `MutationContext` branding and
adds ~500 LOC of boilerplate vs. plain `oxc_allocator`. Use it only if
incremental collection timing becomes necessary.

## Static maps — `phf`

- **Crate:** `phf = { version = "0.11", features = ["macros"] }`.
- **Why:** Compile-time perfect hash maps for the long fixed tables used
  by `builtins/` (operator dispatch, reserved-word sets, intrinsic lookup).
  Zero runtime hashing, zero startup cost.

## Ordered map — `indexmap`

- **Crate:** `indexmap = "2"`.
- **Why:** ECMAScript spec requires object properties enumerate in
  insertion order. `HashMap` does not guarantee this; `IndexMap` does.

## Serialization — `serde`, `serde_json`

- **Crate:** `serde = { version = "1", features = ["derive"] }`,
  `serde_json = "1"`.
- **Why:** Snapshot/dump support and structured test fixtures. Not on the
  hot path.

## URL encoding — `urlencoding`

- **Crate:** `urlencoding = "2"`.
- **Why:** Small, focused. Implements `encodeURI`, `encodeURIComponent`,
  and friends.

## Tracing — `tracing`

- **Crate:** `tracing = "0.1"`.
- **Why:** Structured spans for `eval` and `call`. Compatible with the
  rest of the Quench workspace.

## Regular expressions — `regress`

- **Crate:** `regress = "0.11"`.
- **Why:** Pure-Rust ECMAScript regex engine. Supports the lookbehind and
  backreference semantics that JS `RegExp` requires out of the box. This
  sidesteps the `regex` + `fancy-regex` fallback dance that would otherwise
  be needed to cover ECMAScript patterns.

### Why not `regex` + `fancy-regex`?

- `regex` has no lookbehind or backreferences (the standard finite-automata
  engine cannot express them in linear time).
- `fancy-regex` adds both, but is a backtracking engine, so it does not
  give the `O(m × n)` guarantee `regex` does.
- The fallback is ~10 lines of code, but introduces two engines with
  subtly different error reporting. `regress` is a single engine that
  already accepts ECMAScript patterns verbatim.

### Why not `regex-lite`?

Too minimal — lacks the Unicode properties that JS `RegExp` requires.

## Error handling — `anyhow`

- **Crate:** `anyhow = "1"`.
- **Why:** Internal plumbing only. Builtins throw `JsError` (per the
  AGENTS.md convention); `anyhow` is for top-level glue where we do not
  care to surface structured errors.

## Conformance tooling — `walkdir`, `chrono`

- **Crate:** `walkdir = "2"`, `chrono = "0.4"`.
- **Why:** Used exclusively by the test262 runner
  (`src/test262/runner.rs`) to enumerate the test corpus and timestamp
  checkpoint files. Not part of the runtime path.

### Why not `time`?

`time` is lighter and pure Rust, but `chrono` already covers what the
runner needs (timestamp formatting on the checkpoint file). Switching
would save <2K LOC with no behavioural difference; not worth the churn.

## Dev-only — `tempfile`, `serial_test`

- **Crate:** `tempfile = "3"`, `serial_test = "3"`.
- **Why:** Integration tests need scratch directories; some tests touch
  the `CURRENT_CONTEXT` thread-local and must not run in parallel.

## Integration notes

### `oxc` arena + GC interaction (forward-looking)

The runtime does not yet use a tracing GC. If one is introduced, the
arena (`oxc_allocator::Allocator`) must be kept alive for the duration of
any lowered value that still references OXC `Box<'_, T>` nodes. Two
strategies:

1. Keep the arena alive for the entire execution (simplest, but memory
   grows with the working set).
2. Lower OXC nodes into runtime-owned structs and drop the arena after
   `lower/` finishes.

### Why no `gc`, `lasso`, `slotmap`, `num-bigint`, `lexical`, `rand`, `icu` yet

These are reserved for future stages of test262 conformance and would be
introduced when the corresponding built-ins land:

| Crate       | Triggers when                          |
|-------------|----------------------------------------|
| `gc`        | Object identity GC becomes necessary   |
| `lasso`     | String interning across realms         |
| `slotmap`   | Stable `JsObjectId` handles            |
| `num-bigint`| `BigInt` builtin                       |
| `lexical`   | Hot-path number parsing                |
| `rand`      | `Math.random`                          |
| `icu`       | ECMA-402 `Intl` object                 |

Do not add any of these speculatively — wire them in alongside the
built-in that needs them.
